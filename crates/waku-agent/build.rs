#[path = "../../scripts/build-commit.rs"]
mod build_commit;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_commit::export_commit_sha();
}
