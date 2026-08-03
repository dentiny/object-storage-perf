use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;

#[test]
fn help_describes_the_connectivity_command() {
    cargo_bin_cmd!("object-storage-perf")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("check"))
        .stdout(contains("OSP_ENDPOINT"))
        .stdout(contains("OSP_BUCKET"));
}
