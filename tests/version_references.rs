//! Guards the `dbt-asdl` dependency snippets in `README.md` against version drift.

use semver::{Version, VersionReq};

const README: &str = include_str!("../README.md");
const SNIPPET_PREFIX: &str = "dbt-asdl = \"";

#[test]
fn readme_dependency_snippets_accept_the_crate_version() {
    let crate_version = Version::parse(env!("CARGO_PKG_VERSION")).expect("valid crate version");
    let mut snippets = 0;

    for line in README.lines() {
        let Some((_, rest)) = line.split_once(SNIPPET_PREFIX) else {
            continue;
        };
        let requirement = rest
            .split('"')
            .next()
            .expect("split always yields one element");

        let req = VersionReq::parse(requirement).unwrap_or_else(|err| {
            panic!("README requirement `{requirement}` is not valid semver: {err}")
        });
        assert!(
            req.matches(&crate_version),
            "README snippet `{}` rejects the crate version {crate_version}; \
             use \"{}.{}\"",
            line.trim(),
            crate_version.major,
            crate_version.minor
        );

        snippets += 1;
    }

    assert!(snippets > 0, "no `{SNIPPET_PREFIX}...\"` snippets found in README.md");
}
