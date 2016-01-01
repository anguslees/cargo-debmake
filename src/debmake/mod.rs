use std;
use std::fs;
use std::io;
use std::io::{Read,Write};
use std::env;
use std::collections::BTreeMap;
use std::os::linux::raw::mode_t;
use std::os::unix::fs::OpenOptionsExt;

use rustc_serialize::json::{Json,ToJson};
use time;
use handlebars;
use handlebars::{Handlebars,Context};

use cargo::core::Package;
use cargo::util::{CargoError,CargoResult};
use cargo::Config;

mod helpers;

#[derive(Debug)]
pub struct TemplateError(handlebars::RenderError);
impl std::error::Error for TemplateError {
    fn description(&self) -> &str { self.0.description() }
    fn cause(&self) -> Option<&std::error::Error> { self.0.cause() }
}
impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl From<TemplateError> for Box<CargoError> {
    fn from(t: TemplateError) -> Box<CargoError> { Box::new(t) }
}
impl CargoError for TemplateError {}

struct Template {
    path: String,
    template: String,
    mode: mode_t,
}

fn template_context<'a>(package: &Package, timestamp: &time::Tm) -> CargoResult<Context> {
    let md = package.manifest().metadata();

    let mut ctx = BTreeMap::new();

    ctx.insert("rust_name".to_owned(),
               package.name().to_json());
    ctx.insert("version".to_owned(),
               package.version().to_string().to_json());
    ctx.insert("authors".to_owned(),
               Json::Array(md.authors.iter().cloned().map(|s| s.to_json()).collect()));
    if let Some(ref s) = md.license {
        ctx.insert("license".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.description {
        ctx.insert("description".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.homepage {
        ctx.insert("homepage".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.repository {
        ctx.insert("repository".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.documentation {
        ctx.insert("documentation".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.license_file {
        ctx.insert("license_file".to_owned(), s.to_json());
    }
    if let Some(ref s) = md.readme {
        ctx.insert("readme".to_owned(), s.to_json());
    }

    ctx.insert("depends".to_owned(),
               Json::Array(package.dependencies().iter().map(
                   |d| {
                        let mut h = BTreeMap::new();
                        h.insert("name".to_owned(),
                                 d.name().to_json());
                        h.insert("version_req".to_owned(),
                                 d.version_req().to_string().to_json());
                        h.insert("kind".to_owned(),
                                 format!("{:?}", d.kind()).to_lowercase().to_json());
                        h.insert("optional".to_owned(),
                                 d.is_optional().to_json());
                        h.insert("only_for_platform".to_owned(),
                                 d.only_for_platform().unwrap_or_default().to_json());
                        h.insert("debpkg".to_owned(),
                                 deb_pkgname(d.name(), true).to_json());
                        Json::Object(h)
                    }).collect()));

    if let Some(ref s) = md.license_file {
        let mut contents = String::new();
        if fs::File::open(s).and_then(|mut f| f.read_to_string(&mut contents)).is_ok() {
            ctx.insert("license_contents".to_owned(), contents.to_json());
        }
    }
    ctx.insert("rfc822date".to_owned(),
               time::strftime("%a, %d %b %Y %T %z", &timestamp).unwrap().to_json());
    ctx.insert("deb_srcpkg".to_owned(),
               deb_pkgname(package.name(), false).to_json());
    ctx.insert("deb_binpkg".to_owned(),
               deb_pkgname(package.name(), true).to_json());
    ctx.insert("deb_version".to_owned(),
               format!("{}-1", package.version()).to_json());
    let username = try!(get_username());
    ctx.insert("deb_maint".to_owned(),
               username.to_json());
    ctx.insert("deb_email".to_owned(),
               env::var("DEBEMAIL")
               .or(env::var("EMAIL"))
               .unwrap_or("<you>@debian.org".to_owned()).to_json());

    Ok(Context::wraps(&ctx))
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
                path: concat!("debian/", $e).to_owned(),
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

    let mut handlebars = Handlebars::new();
    helpers::add_helpers(&mut handlebars);

    for t in templates.iter() {
        handlebars.register_template_string(&t.path, t.template.clone())
            .expect(&t.path);
    }

    let context = try!(template_context(package, timestamp));

    for t in templates.iter() {
        try!(config.shell().status("Generating", &t.path));
        let path = package.root().join(&t.path);

        if let Some(p) = path.parent() {
            try!(fs::create_dir_all(p));
        }

        let mut f = try!(fs::OpenOptions::new()
                         .write(true)
                         .create(true)
                         .truncate(true)
                         .mode(t.mode)
                         .open(&path));
        try!(handlebars.renderw(&t.path, &context, &mut f)
             .map_err(TemplateError));
    }

    Ok(())
}
