extern crate cargo;
extern crate rustc_serialize;
extern crate liquid;
extern crate time;
extern crate libc;

use std::fs;
use std::path::Path;

use cargo::sources::{PathSource,RegistrySource};
use cargo::{Config,CliResult};
use cargo::util::{important_paths,CargoResult,human,ChainError,without_prefix};
use cargo::core::{SourceId,Dependency,Package};
use cargo::core::registry::Registry;
use cargo::core::source::Source;

mod debmake;

const USAGE: &'static str = r#"
Generate Debian packaging boilerplate for a Cargo package

Usage:
    cargo debmake [options]

Options:
    -h, --help                  Print this message
    -v, --verbose               Use verbose output
    -q, --quiet                 No output printed to stdout
    -d, --download CRATE        Download and operate on latest version of CRATE
"#;

#[derive(RustcDecodable)]
struct Options {
    flag_verbose: bool,
    flag_quiet: bool,
    flag_download: Option<String>,
}

fn main() {
    cargo::execute_main_without_stdin(real_main, false, USAGE)
}

fn download(krate: &str, version: Option<&str>, config: &Config) -> CargoResult<Package> {
    let source = try!(SourceId::for_central(&config));
    let mut registry = RegistrySource::new(&source, config);
    let dep = try!(Dependency::parse(krate, version, &source));
    let summaries = try!(registry.query(&dep));
    let pkgid = try!(summaries.iter()
                     .map(|s| s.package_id())
                     .max()
                     .ok_or_else(|| human(format!("Failed to find {} in {}", krate, source))));
    let pkgids = [pkgid.clone()];
    try!(registry.download(&pkgids));
    let mut pkgs = try!(registry.get(&pkgids));
    Ok(pkgs.remove(0))
}

fn copy_files(pkg: &Package, new_root: &Path, config: &Config) -> CargoResult<()> {
    try!(config.shell().status("Copying", format!("{} -> {}", pkg, new_root.to_string_lossy())));
    let mut src = try!(PathSource::for_path(pkg.root(), config));
    let list = {
        let new_pkg = try!(src.root_package());
        try!(src.list_files(&new_pkg))
    };
    for file in list.iter() {
        let rel_path = without_prefix(file, pkg.root()).unwrap();
        let new_file = new_root.join(rel_path);

        try!(fs::create_dir_all(new_file.parent().unwrap()));
        try!(fs::copy(file, new_file));
    }
    Ok(())
}

fn real_main(options: Options, config: &Config) -> CliResult<Option<()>> {
    try!(config.shell().set_verbosity(options.flag_verbose, options.flag_quiet));

    let rootdir = {
        if let Some(ref krate) = options.flag_download {
            let pkg = try!(download(krate, None, config));
            let path = config.cwd().join(format!("{}-{}", pkg.name(), pkg.version()));
            try!(fs::create_dir(&path).chain_error(
                || human(format!("Failed to create '{}'", path.to_string_lossy()))));
            try!(copy_files(&pkg, &path, config));
            path
        } else {
            let root = try!(important_paths::find_root_manifest_for_cwd(None));
            root.parent().unwrap().to_path_buf()
        }
    };

    let mut source = try!(PathSource::for_path(&rootdir, config));
    let package = try!(source.root_package());

    let timestamp = time::now();

    try!(debmake::debmake(&package, &timestamp, config));
    Ok(None)
}
