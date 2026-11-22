use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use libcontainer::container::Container;
use libcontainer::container::builder::ContainerBuilder;
use libcontainer::oci_spec::runtime::{
    Capability, Linux, LinuxCapabilitiesBuilder, MountBuilder, PosixRlimitBuilder, PosixRlimitType,
    Process, Root, Spec,
};
use libcontainer::syscall::syscall::SyscallType;
use nix::sys::wait::{WaitStatus, waitpid};

use crate::store::Store;
use crate::types::{InstanceInfo, InstanceMeta, InstanceRequest};
use crate::unpack::restore_modes;

pub(crate) const DEFAULT_PATH: &str =
    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub fn runtime_state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir).join("phprt/buildimages/state");
    }
    let uid = nix::unistd::getuid().as_raw();
    std::env::temp_dir().join(format!("phprt-buildimages-{uid}"))
}

pub fn ensure_instance(store: &Store, request: &InstanceRequest) -> Result<InstanceInfo> {
    let dir = store.instances_dir().join(&request.id);
    let bundle = dir.join("bundle");
    let rootfs = bundle.join("rootfs");
    let meta_path = dir.join("instance.json");
    let project = request.project_root.to_string_lossy().to_string();
    let executable = executable_id(&request.executable)?;
    let mounts = mount_id(&request.mounts);

    let previous = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|text| serde_json::from_str::<InstanceMeta>(&text).ok());

    let same_base = previous.as_ref().is_some_and(|meta| {
        meta.image == request.image && meta.project_root == project && rootfs.is_dir()
    });
    if dir.exists() && !same_base {
        delete_instance(&request.id).ok();
        fs::remove_dir_all(&dir).with_context(|| format!("删除旧实例 {}", dir.display()))?;
    } else if same_base {
        let changed = previous
            .as_ref()
            .is_some_and(|meta| meta.executable != executable || meta.mounts != mounts);
        if changed {
            // rootfs 里的工具还能用，只重建运行实例。
            delete_instance(&request.id).ok();
        }
    }
    fs::create_dir_all(&bundle)?;
    if !rootfs.join("usr").is_dir() {
        let source = store.image_dir(&request.image).join("rootfs");
        if !source.is_dir() {
            bail!(
                "镜像 {} 的 rootfs 不存在；先执行 phprt buildimages add",
                crate::store::short_digest(&request.image)
            );
        }
        copy_tree(&source, &rootfs)
            .with_context(|| format!("复制 rootfs 到 {}", rootfs.display()))?;
        restore_modes(&store.image_dir(&request.image), &rootfs)?;
    }

    let spec = build_spec(request, &rootfs)?;
    spec.save(bundle.join("config.json"))?;
    let meta = InstanceMeta {
        id: request.id.clone(),
        image: request.image.clone(),
        project_root: project,
        created: chrono::Utc::now().to_rfc3339(),
        executable,
        mounts,
    };
    crate::store::write_atomic(&meta_path, &serde_json::to_vec_pretty(&meta)?)?;

    ensure_container(&request.id, &bundle)?;
    Ok(InstanceInfo {
        id: request.id.clone(),
        image: request.image.clone(),
        project_root: request.project_root.clone(),
        bundle,
        rootfs,
    })
}

fn executable_id(path: &Path) -> Result<String> {
    let meta = fs::metadata(path)
        .with_context(|| format!("读不到 phprt 可执行文件 {}", path.display()))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Ok(format!(
        "{}:{}:{}",
        canonical.display(),
        meta.len(),
        modified
    ))
}

fn mount_id(mounts: &[crate::types::MountSpec]) -> Vec<String> {
    mounts
        .iter()
        .map(|mount| {
            format!(
                "{}:{}:{}",
                mount.source.display(),
                mount.destination.display(),
                mount.readonly
            )
        })
        .collect()
}

fn ensure_container(id: &str, bundle: &Path) -> Result<()> {
    let state = runtime_state_dir();
    fs::create_dir_all(&state)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&state)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&state, perms)?;
    }
    let container_dir = state.join(id);
    let pid_file = state.join(format!("{id}.pid"));

    if container_dir.exists() {
        let running = Container::load(container_dir.clone())
            .ok()
            .and_then(|mut container| {
                container.refresh_status().ok()?;
                Some(matches!(
                    container.status(),
                    libcontainer::container::ContainerStatus::Running
                ))
            })
            .unwrap_or(false);
        if running {
            return Ok(());
        }
        if let Ok(mut container) = Container::load(container_dir.clone()) {
            let _ = container.delete(true);
        }
        let _ = fs::remove_dir_all(&container_dir);
        let _ = fs::remove_file(&pid_file);
    }

    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let mut builder = ContainerBuilder::new(id.to_string(), SyscallType::default())
        .with_root_path(&state)?
        .with_pid_file(Some(&pid_file))?;
    builder.stdin = Some(null.try_clone()?.into());
    builder.stdout = Some(null.try_clone()?.into());
    builder.stderr = Some(null.into());
    let mut container = builder
        .as_init(bundle)
        .with_systemd(true)
        .with_detach(true)
        .build()
        .context("创建构建容器失败")?;
    if let Err(err) = container.start() {
        let _ = container.delete(true);
        let _ = fs::remove_dir_all(&container_dir);
        let _ = fs::remove_file(&pid_file);
        return Err(err).context("启动构建容器失败");
    }
    Ok(())
}

fn build_spec(request: &InstanceRequest, rootfs: &Path) -> Result<Spec> {
    let euid = nix::unistd::geteuid().as_raw();
    let egid = nix::unistd::getegid().as_raw();

    let mut mounts = libcontainer::oci_spec::runtime::get_rootless_mounts();
    let mut destinations: HashSet<PathBuf> =
        mounts.iter().map(|m| m.destination().clone()).collect();
    let mut add = |source: &Path, destination: &Path, readonly: bool| -> Result<()> {
        if !destinations.insert(destination.to_path_buf()) {
            return Ok(());
        }
        let options = if readonly {
            vec!["rbind".into(), "ro".into()]
        } else {
            vec!["rbind".into(), "rw".into()]
        };
        let mount = MountBuilder::default()
            .destination(destination.to_path_buf())
            .typ("none")
            .source(source.to_path_buf())
            .options(options)
            .build()
            .map_err(|e| anyhow!("构造挂载失败：{e}"))?;
        mounts.push(mount);
        Ok(())
    };

    for mount in &request.mounts {
        add(&mount.source, &mount.destination, mount.readonly)?;
    }
    add(&request.executable, Path::new("/phprt"), true)?;
    let resolv = Path::new("/etc/resolv.conf");
    if resolv.exists() {
        add(resolv, resolv, true)?;
    }

    let caps: HashSet<Capability> = [
        Capability::AuditWrite,
        Capability::Chown,
        Capability::DacOverride,
        Capability::Fowner,
        Capability::Fsetid,
        Capability::Kill,
        Capability::Mknod,
        Capability::NetBindService,
        Capability::NetRaw,
        Capability::Setfcap,
        Capability::Setgid,
        Capability::Setuid,
        Capability::Setpcap,
        Capability::SysChroot,
    ]
    .into_iter()
    .collect();

    let mut process = Process::default();
    process
        .set_args(Some(vec!["sleep".into(), "infinity".into()]))
        .set_env(Some(vec![
            format!("PATH={DEFAULT_PATH}"),
            "HOME=/root".into(),
            "TERM=xterm".into(),
        ]))
        .set_cwd(PathBuf::from("/"))
        .set_capabilities(Some(
            LinuxCapabilitiesBuilder::default()
                .bounding(caps.clone())
                .effective(caps.clone())
                .inheritable(caps.clone())
                .permitted(caps.clone())
                .ambient(caps.clone())
                .build()?,
        ))
        .set_no_new_privileges(Some(false))
        .set_rlimits(Some(vec![
            PosixRlimitBuilder::default()
                .typ(PosixRlimitType::RlimitNofile)
                .hard(65536u64)
                .soft(65536u64)
                .build()?,
        ]));

    let mut root = Root::default();
    root.set_path(rootfs.to_path_buf())
        .set_readonly(Some(false));

    let mut spec = Spec::default();
    spec.set_version("1.0.2".to_string());
    spec.set_process(Some(process));
    spec.set_hostname(Some("phprt-build".to_string()));
    spec.set_root(Some(root));
    spec.set_mounts(Some(mounts));
    spec.set_linux(Some(Linux::rootless(euid, egid)));
    Ok(spec)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let meta = fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&from)?;
            symlink(target, &to)?;
        } else if meta.is_dir() {
            copy_tree(&from, &to)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = meta.permissions();
            perms.set_mode(meta.permissions().mode());
            fs::set_permissions(&to, perms)?;
        } else if meta.file_type().is_file() {
            fs::copy(&from, &to)?;
            fs::set_permissions(&to, meta.permissions())?;
        }
    }
    Ok(())
}

/// 在构建实例里执行一条命令。必须在单线程进程里调用（会先 setns user namespace）。
pub fn exec(
    id: &str,
    workdir: &Path,
    mut env: HashMap<String, String>,
    command: Vec<String>,
) -> Result<i32> {
    if command.is_empty() {
        bail!("容器内命令为空");
    }
    let state = runtime_state_dir();
    let container_dir = state.join(id);
    let mut container = Container::load(container_dir.clone())
        .with_context(|| format!("没有构建实例 {id}（{}）", container_dir.display()))?;
    container.refresh_status()?;
    if !matches!(
        container.status(),
        libcontainer::container::ContainerStatus::Running
    ) {
        bail!("构建实例 {id} 没有运行");
    }
    let pid = container
        .pid()
        .ok_or_else(|| anyhow!("构建实例 {id} 没有 pid"))?;
    enter_user_ns(pid.as_raw())?;

    env.entry("PATH".to_string())
        .or_insert_with(|| DEFAULT_PATH.to_string());
    env.entry("HOME".to_string())
        .or_insert_with(|| "/root".to_string());
    env.entry("TERM".to_string())
        .or_insert_with(|| "xterm".to_string());

    let mut builder = ContainerBuilder::new(id.to_string(), SyscallType::default())
        .with_root_path(&state)?
        .with_pid_file(None::<PathBuf>)?;
    builder.stdin = Some(std::io::stdin().as_fd().try_clone_to_owned()?);
    builder.stdout = Some(std::io::stdout().as_fd().try_clone_to_owned()?);
    builder.stderr = Some(std::io::stderr().as_fd().try_clone_to_owned()?);
    let child = builder
        .validate_id()?
        .as_tenant()
        .with_detach(false)
        .with_cwd(Some(workdir.to_path_buf()))
        .with_env(env)
        .with_container_args(command)
        .with_user(Some(0))
        .with_group(Some(0))
        .build()
        .context("在构建实例里执行命令失败")?;
    match waitpid(child, None)? {
        WaitStatus::Exited(_, code) => Ok(code),
        WaitStatus::Signaled(_, signal, _) => Ok(128 + signal as i32),
        _ => Ok(0),
    }
}

fn enter_user_ns(pid: i32) -> Result<()> {
    let path = format!("/proc/{pid}/ns/user");
    let file = File::open(&path).with_context(|| format!("打开 {path} 失败"))?;
    let rc = unsafe { libc::setns(file.as_raw_fd(), libc::CLONE_NEWUSER) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("加入构建实例的 user namespace 失败（{path}）"));
    }
    Ok(())
}

pub fn delete_instance(id: &str) -> Result<()> {
    let state = runtime_state_dir();
    let container_dir = state.join(id);
    if container_dir.exists()
        && let Ok(mut container) = Container::load(container_dir.clone())
    {
        let _ = container.delete(true);
    }
    let _ = fs::remove_dir_all(&container_dir);
    let _ = fs::remove_file(state.join(format!("{id}.pid")));
    Ok(())
}
