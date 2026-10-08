//! Platform build metadata: the checked-out commit, republished so the
//! daemon's hello handshake can report what it was built from.

#[path = "../../scripts/build-commit.rs"]
mod build_commit;
mod native_whistle;

fn main() {
    native_whistle::configure();
    println!("cargo:rerun-if-changed=build.rs");
    build_commit::export_commit_sha();
}
