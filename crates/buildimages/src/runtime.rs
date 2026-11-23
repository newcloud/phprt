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
use crate::unpack::{make_removable, restore_modes};

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

    let existed = dir.exists();
    fs::create_dir_all(&dir)?;
    let previous = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|text| serde_json::from_str::<InstanceMeta>(&text).ok());

    if let Some(meta) = &previous {
        if meta.image != request.image {
            bail!(
                "实例 {} 已绑定镜像 {}，不能换成 {}。\n  \
                 换镜像请用 --instance-name <新名字> --image <镜像> 新建实例",
                meta.display_name(),
                crate::store::short_digest(&meta.image),
                crate::store::short_digest(&request.image)
            );
        }
        if !meta.name.is_empty() && meta.name != request.name {
            bail!(
                "实例目录 {} 里的名字是 {}，不是 {}；请用对应名字或换一个实例",
                request.id,
                meta.name,
                request.name
            );
        }
    } else if existed && rootfs.join("usr").is_dir() {
        bail!(
            "实例目录 {} 存在 rootfs 但缺少 instance.json；\
             先执行 phprt buildimages rm-instance <名字> 或手工清理该目录",
            dir.display()
        );
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

    if let Some(meta) = &previous
        && (meta.executable != executable || meta.mounts != mounts)
    {
        // rootfs 里的工具还能用，只重建运行实例。
        delete_instance(&request.id)
            .with_context(|| format!("重建实例 {} 的运行时容器失败", request.name))?;
    }

    let spec = build_spec(request, &rootfs)?;
    spec.save(bundle.join("config.json"))?;

    let mut projects = previous
        .as_ref()
        .map(InstanceMeta::project_roots)
        .unwrap_or_default();
    if !projects.iter().any(|item| item == &project) {
        projects.push(project.clone());
    }
    let owner = previous
        .as_ref()
        .map(|meta| meta.owner.clone())
        .filter(|owner| !owner.is_empty())
        .unwrap_or_else(|| project.clone());
    let created = previous
        .as_ref()
        .map(|meta| meta.created.clone())
        .filter(|created| !created.is_empty())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let image_ref = if request.image_ref.is_empty() {
        previous
            .as_ref()
            .map(|meta| meta.image_ref.clone())
            .unwrap_or_default()
    } else {
        request.image_ref.clone()
    };
    let image_ref = if image_ref.is_empty() {
        store
            .resolve(&request.image)
            .map(|image| image.reference)
            .unwrap_or_default()
    } else {
        image_ref
    };
    let meta = InstanceMeta {
        id: request.id.clone(),
        name: request.name.clone(),
        image: request.image.clone(),
        image_ref,
        owner,
        projects,
        created,
        executable,
        mounts,
    };
    crate::store::write_atomic(&meta_path, &serde_json::to_vec_pretty(&meta)?)?;

    ensure_container(&request.id, &bundle)?;
    Ok(InstanceInfo {
        id: request.id.clone(),
        name: request.name.clone(),
        image: request.image.clone(),
        image_ref: meta.image_ref.clone(),
        owner: meta.owner.clone(),
        projects: meta.projects.clone(),
        created: meta.created.clone(),
        bundle,
        rootfs,
    })
}

/// 实例锁；drop 时关闭文件，flock 由内核释放。
pub struct InstanceLock {
    _file: File,
}

/// 尝试拿实例锁；实例正被另一个 phprt 使用时返回 None。
pub fn try_lock_instance(store: &Store, id: &str) -> Result<Option<InstanceLock>> {
    let dir = store.instance_dir(id);
    fs::create_dir_all(&dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(".lock"))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(Some(InstanceLock { _file: file }));
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(None),
        _ => Err(err).with_context(|| format!("锁定实例 {id} 失败")),
    }
}

/// 阻塞等待实例锁。
pub fn lock_instance(store: &Store, id: &str) -> Result<InstanceLock> {
    let dir = store.instance_dir(id);
    fs::create_dir_all(&dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(".lock"))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("等待实例 {id} 空闲失败"));
    }
    Ok(InstanceLock { _file: file })
}

/// 运行时容器是否正在 running。只用于展示，不参与判断。
pub fn is_running(id: &str) -> bool {
    let container_dir = runtime_state_dir().join(id);
    Container::load(container_dir)
        .ok()
        .and_then(|mut container| {
            container.refresh_status().ok()?;
            Some(matches!(
                container.status(),
                libcontainer::container::ContainerStatus::Running
            ))
        })
        .unwrap_or(false)
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
    fs::create_dir_all(destination)?;
    // 上一次复制可能把目录恢复成了 0555；先放宽，复制完再按记录恢复。
    make_removable(destination)?;
    let mut links = HashMap::new();
    copy_tree_inner(source, destination, &mut links)
}

fn copy_tree_inner(
    source: &Path,
    destination: &Path,
    links: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, symlink};

    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let meta = fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            let key = (meta.dev(), meta.ino());
            if meta.nlink() > 1
                && let Some(first) = links.get(&key).cloned()
            {
                remove_existing(&to)?;
                fs::hard_link(&first, &to)?;
                set_mtime_symlink(&to, &meta)?;
                continue;
            }
            let target = fs::read_link(&from)?;
            remove_existing(&to)?;
            symlink(target, &to)?;
            set_mtime_symlink(&to, &meta)?;
            if meta.nlink() > 1 {
                links.insert(key, to);
            }
        } else if meta.is_dir() {
            copy_tree_inner(&from, &to, links)?;
            fs::set_permissions(&to, meta.permissions())?;
            set_mtime(&to, &meta)?;
        } else if meta.file_type().is_file() {
            let key = (meta.dev(), meta.ino());
            if meta.nlink() > 1
                && let Some(first) = links.get(&key).cloned()
            {
                remove_existing(&to)?;
                fs::hard_link(&first, &to)?;
                // 同一个 inode 的 mtime 与第一个路径一致，重复设置无害。
                set_mtime(&to, &meta)?;
                continue;
            }
            remove_existing(&to)?;
            fs::copy(&from, &to)?;
            fs::set_permissions(&to, meta.permissions())?;
            set_mtime(&to, &meta)?;
            if meta.nlink() > 1 {
                links.insert(key, to);
            }
        }
    }
    Ok(())
}

fn remove_existing(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            make_removable(path)?;
            fs::remove_dir_all(path)?;
        }
        Ok(_) => fs::remove_file(path)?,
        Err(_) => {}
    }
    Ok(())
}

fn set_mtime(path: &Path, meta: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let nanos = u32::try_from(meta.mtime_nsec()).unwrap_or(0);
    let time = filetime::FileTime::from_unix_time(meta.mtime(), nanos);
    filetime::set_file_times(path, time, time)
        .with_context(|| format!("设置 {} 的 mtime", path.display()))
}

fn set_mtime_symlink(path: &Path, meta: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let nanos = u32::try_from(meta.mtime_nsec()).unwrap_or(0);
    let time = filetime::FileTime::from_unix_time(meta.mtime(), nanos);
    filetime::set_symlink_file_times(path, time, time)
        .with_context(|| format!("设置 {} 的 mtime", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn copy_tree_preserves_hardlinks_and_mtime() {
        let root = std::env::temp_dir().join(format!("phprt-copy-tree-{}", std::process::id()));
        let _ = make_removable(&root);
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("sub")).unwrap();
        fs::write(source.join("a"), b"data").unwrap();
        fs::hard_link(source.join("a"), source.join("sub/b")).unwrap();
        std::os::unix::fs::symlink("a", source.join("s1")).unwrap();
        fs::hard_link(source.join("s1"), source.join("s2")).unwrap();
        let old = filetime::FileTime::from_unix_time(1_000_000_000, 0);
        filetime::set_file_times(source.join("a"), old, old).unwrap();
        filetime::set_file_times(source.join("sub"), old, old).unwrap();
        filetime::set_symlink_file_times(source.join("s1"), old, old).unwrap();

        copy_tree(&source, &destination).unwrap();

        let a = fs::metadata(destination.join("a")).unwrap();
        let b = fs::metadata(destination.join("sub/b")).unwrap();
        assert_eq!(a.ino(), b.ino());
        assert_eq!(a.nlink(), 2);
        assert_eq!(a.mtime(), 1_000_000_000);
        assert_eq!(
            fs::metadata(destination.join("sub")).unwrap().mtime(),
            1_000_000_000
        );
        let s1 = fs::symlink_metadata(destination.join("s1")).unwrap();
        let s2 = fs::symlink_metadata(destination.join("s2")).unwrap();
        assert!(s1.file_type().is_symlink());
        assert_eq!(s1.ino(), s2.ino());
        assert_eq!(s1.nlink(), 2);
        assert_eq!(s1.mtime(), 1_000_000_000);

        let _ = make_removable(&root);
        let _ = fs::remove_dir_all(&root);
    }
}
