fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    let git_head = std::fs::read_to_string("../.git/HEAD").unwrap();
    if git_head.starts_with("ref: ") {
        let git_ref = &git_head.strip_prefix("ref: ").unwrap().strip_suffix("\n").unwrap();
        println!("cargo:rerun-if-changed=.git/{git_ref}");
        let git_hash = std::fs::read_to_string(format!("../.git/{git_ref}")).unwrap();
        println!("cargo:rustc-env=GIT_HASH={git_hash}");
    } else {
        println!("cargo:rustc-env=GIT_HASH={git_head}");
    }
}
