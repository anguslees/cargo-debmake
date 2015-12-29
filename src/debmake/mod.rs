use std;
use std::fs;
use std::path::PathBuf;
use std::default::Default;
use std::io;
use std::io::{Read,Write};
use std::env;
use std::collections::HashMap;
use std::os::linux::raw::mode_t;
use std::os::unix::fs::OpenOptionsExt;

use time;

use liquid;
use liquid::{Value,Context,Renderable};

use cargo::core::Package;
use cargo::util::{CargoError,CargoResult};
use cargo::Config;

mod filters;

#[derive(Debug)]
pub struct LiquidError(liquid::Error);
impl std::error::Error for LiquidError {
    fn description(&self) -> &str { self.0.description() }
    fn cause(&self) -> Option<&std::error::Error> { self.0.cause() }
}
impl std::fmt::Display for LiquidError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl From<LiquidError> for Box<CargoError> {
    fn from(t: LiquidError) -> Box<CargoError> { Box::new(t) }
}
impl CargoError for LiquidError {}

struct Template {
    path: PathBuf,
    template: String,
    mode: mode_t,
}

fn template_context<'a>(package: &Package, timestamp: &time::Tm) -> CargoResult<Context<'a>> {
    let md = package.manifest().metadata();

    let mut ctx = Context::new();
    filters::add_filters(&mut ctx);

    ctx.set_val("rust_name",
                Value::Str(package.name().to_owned()));
    ctx.set_val("version",
                Value::Str(package.version().to_string()));
    ctx.set_val("authors",
                Value::Array(md.authors.iter().cloned().map(Value::Str).collect()));
    if let Some(ref s) = md.license {
        ctx.set_val("license", Value::Str(s.clone()));
    }
    if let Some(ref s) = md.description {
        ctx.set_val("description", Value::Str(s.trim().to_string()));
    }
    if let Some(ref s) = md.homepage {
        ctx.set_val("homepage", Value::Str(s.clone()));
    }
    if let Some(ref s) = md.repository {
        ctx.set_val("repository", Value::Str(s.clone()));
    }
    if let Some(ref s) = md.documentation {
        ctx.set_val("documentation", Value::Str(s.clone()));
    }
    if let Some(ref s) = md.license_file {
        ctx.set_val("license_file", Value::Str(s.clone()));
    }
    if let Some(ref s) = md.readme {
        ctx.set_val("readme", Value::Str(s.clone()));
    }

    ctx.set_val("depends",
                Value::Array(package.dependencies().iter().map(
                    |d| {
                        let mut h = HashMap::new();
                        h.insert("name".to_owned(),
                                 Value::Str(d.name().to_owned()));
                        h.insert("version_req".to_owned(),
                                 Value::Str(d.version_req().to_string()));
                        h.insert("kind".to_owned(),
                                 Value::Str(format!("{:?}", d.kind()).to_lowercase()));
                        h.insert("optional".to_owned(),
                                 Value::Str(format!("{}", d.is_optional())));
                        h.insert("only_for_platform".to_owned(),
                                 Value::Str(d.only_for_platform().unwrap_or_default().to_owned()));
                        h.insert("debpkg".to_owned(),
                                 Value::Str(deb_pkgname(d.name(), true)));
                        Value::Object(h)
                    }).collect()));

    if let Some(ref s) = md.license_file {
        let mut contents = String::new();
        if fs::File::open(s).and_then(|mut f| f.read_to_string(&mut contents)).is_ok() {
            ctx.set_val("license_contents", Value::Str(contents));
        }
    }
    ctx.set_val("rfc822date",
                Value::Str(time::strftime("%a, %d %b %Y %T %z", &timestamp).unwrap()));
    ctx.set_val("deb_srcpkg",
                Value::Str(deb_pkgname(package.name(), false)));
    ctx.set_val("deb_binpkg",
                Value::Str(deb_pkgname(package.name(), true)));
    ctx.set_val("deb_version",
                Value::Str(format!("{}-1", package.version())));
    let username = try!(get_username());
    ctx.set_val("deb_maint",
                Value::Str(username));
    ctx.set_val("deb_email",
                Value::Str(env::var("DEBEMAIL")
                           .or(env::var("EMAIL"))
                           .unwrap_or("<you>@debian.org".to_owned())));

    Ok(ctx)
}

#[cfg(unix)]
fn get_username() -> CargoResult<String> {
    // TODO: find/use some existing library routine instead
    use libc;
    use std::ptr;
    use std::ffi::CStr;

    let gecos_res = unsafe {
        let bufsize = match libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) {
            -1 => 16384, // "Should be more than enough"
            v => v as usize,
        };

        let mut buf = vec![0; bufsize];
        let mut pwdbuf = libc::passwd{
            pw_name: ptr::null_mut(),
            pw_passwd: ptr::null_mut(),
            pw_uid: 0,
            pw_gid: 0,
            pw_gecos: ptr::null_mut(),
            pw_dir: ptr::null_mut(),
            pw_shell: ptr::null_mut(),
        };
        let mut result = ptr::null_mut();  // This has the lifetime of pwdbuf

        match libc::getpwuid_r(libc::getuid(), &mut pwdbuf, buf.as_mut_ptr(), buf.len(),
                               &mut result) {
            0 if !result.is_null() => {
                // Copy pw_gecos CStr into a regular Rust-managed String
                Ok(CStr::from_ptr((*result).pw_gecos).to_string_lossy().to_owned())
            },
            0 => {
                // Not found
                Err(io::Error::from_raw_os_error(libc::ENOENT))
            },
            errno => {
                Err(io::Error::from_raw_os_error(errno))
            },
        }
    };

    gecos_res
        .map(|v| v.split(',').next().unwrap().to_owned())
        .map_err(|e| e.into())
}

fn deb_pkgname(rust_name: &str, is_lib: bool) -> String {
    // https://www.debian.org/doc/debian-policy/ch-controlfields.html#s-f-Source
    // Valid package names are /[a-z0-9][a-z0-9.+-]+/
    let name = rust_name.chars().map(
        |c| match c {
            'a'...'z' | '0'...'9' | '.' | '+' | '-' => c,
            'A'...'Z' => c.to_lowercase().next().unwrap(),
            _ => '-',
        }).collect();
    if is_lib {
        format!("librust-{}-dev", name)
    } else {
        name
    }
}

pub fn debmake(package: &Package, timestamp: &time::Tm, config: &Config) -> CargoResult<()> {
    macro_rules! deb_template {
        ($e:expr) => (
            Template{
                path: PathBuf::from(concat!("debian/", $e)),
                template: include_str!(concat!("../templates/", $e)).to_owned(),
                mode: 0o666,
            })
    }

    let templates = [
        deb_template!("changelog"),
        deb_template!("compat"),
        deb_template!("control"),
        deb_template!("copyright"),
        Template{mode: 0o777, .. deb_template!("rules")},
        deb_template!("watch"),
        ];

    for t in templates.iter() {
        try!(config.shell().status("Generating", &t.path.to_string_lossy()));

        let mut options = Default::default();
        let mut context = try!(template_context(package, timestamp));

        let output = try!(
            liquid::parse(&t.template, &mut options)
                .and_then(|tmpl| tmpl.render(&mut context))
                .map_err(LiquidError));

        let output = match output {
            Some(v) => v,
            None => continue,
        };

        let path = package.root().join(&t.path);

        if let Some(p) = path.parent() {
            try!(fs::create_dir_all(p));
        }

        let mut f = try!(fs::OpenOptions::new()
                         .write(true)
                         .create(true)
                         .mode(t.mode)
                         .open(&path));
        try!(f.write_all(output.as_bytes()));
    }

    Ok(())
}
