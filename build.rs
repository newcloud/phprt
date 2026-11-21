fn main() {
    // option_env!("PHPRT_CATALOG_URL") 在编译时把发布地址写进二进制。
    // 显式声明依赖，改地址后重新 cargo build 不会继续用旧值。
    println!("cargo:rerun-if-env-changed=PHPRT_CATALOG_URL");
}
