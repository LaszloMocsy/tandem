fn main() {
    // The build environment is compiled in (see src/build_env.rs), so a typo
    // fails the build instead of shipping an unlabelled pre-release.
    println!("cargo:rerun-if-env-changed=TANDEM_ENV");
    if let Ok(env) = std::env::var("TANDEM_ENV")
        && !matches!(env.as_str(), "" | "alpha" | "beta")
    {
        panic!("TANDEM_ENV must be alpha, beta or unset, not {env:?}");
    }
    tauri_build::build()
}
