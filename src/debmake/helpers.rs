use std::io::BufRead;

use regex::Regex;
use handlebars::{Handlebars,RenderError,Helper,Context,RenderContext,Renderable,JsonRender};

/// Reformat string as per Debian "description" format.
fn deb_description(c: &Context, h: &Helper, r: &Handlebars, rc: &mut RenderContext) -> Result<(), RenderError> {
    if let Some(ref t) = h.template() {
        let mut buf = Vec::with_capacity(1024);
        {
            let mut new_rc = rc.with_writer(&mut buf);
            try!(t.render(c, r, &mut new_rc));
        }
        for line in buf.lines() {
            match try!(line).trim_right() {
                "" => {
                    try!(rc.writer.write(b" .\n"));
                },
                x => {
                    try!(rc.writer.write(b" "));
                    try!(rc.writer.write(x.as_bytes()));
                    try!(rc.writer.write(b"\n"));
                },
            }
        }
    }
    Ok(())
}

/// Strip newline characters.
fn strip_newlines(c: &Context, h: &Helper, r: &Handlebars, rc: &mut RenderContext) -> Result<(), RenderError> {
    if let Some(ref t) = h.template() {
        let mut buf = Vec::with_capacity(1024);
        {
            let mut new_rc = rc.with_writer(&mut buf);
            try!(t.render(c, r, &mut new_rc));
        }
        let mut first = true;
        for line in buf.split(|&c| c == b'\n') {
            if first {
                first = false;
            } else {
                try!(rc.writer.write(b" "));
            }
            try!(rc.writer.write(line));
        }
    }
    Ok(())
}

/// Return truthy if value of arg2 matches regex literal arg1.
fn matches(c: &Context, h: &Helper, r: &Handlebars, rc: &mut RenderContext) -> Result<(), RenderError> {
    let regex_param = try!(h.param(0).ok_or_else(
        || RenderError::new("Regex param not found for helper \"matches\"")));
    let value_param = try!(h.param(1).ok_or_else(
        || RenderError::new("Value param not found for helper \"matches\"")));

    let regex = try!(Regex::new(&regex_param)
                     .map_err(|e| RenderError::new(format!("Invalid regex: {}", e))));

    let value = c.navigate(rc.get_path(), value_param).render();

    let tmpl = if regex.is_match(&value) { h.template() } else { h.inverse() };
    if let Some(ref t) = tmpl {
        try!(t.render(c, r, rc));
    }

    Ok(())
}

#[test]
fn test_matches() {
    use std::collections::BTreeMap;
    use handlebars::Template;

    let t0 = Template::compile("{{#matches ba[rR] foo}}yes{{else}}no{{/matches}}".to_owned())
        .expect("t0 template");
    let t1 = Template::compile(r"{{#matches ^a foo}}yes{{else}}no{{/matches}}".to_owned())
        .expect("t1 template");
    let t2 = Template::compile(r"{{#matches a foo}}yes{{/matches}}".to_owned())
        .expect("t2 template");

    let mut handlebars = Handlebars::new();
    add_helpers(&mut handlebars);
    handlebars.register_template("t0", t0);
    handlebars.register_template("t1", t1);
    handlebars.register_template("t2", t2);

    let mut m = BTreeMap::new();
    m.insert("foo".to_owned(), "bar".to_owned());

    let r0 = handlebars.render("t0", &m).expect("r0 render");
    assert_eq!(r0, "yes");

    let r1 = handlebars.render("t1", &m).expect("r1 render");
    assert_eq!(r1, "no");

    let r2 = handlebars.render("t2", &m).expect("r2 render");
    assert_eq!(r2, "yes");
}

pub fn add_helpers(r: &mut Handlebars) {
    macro_rules! add_helper {
        ($e:expr) => (r.register_helper(stringify!($e), Box::new($e)))
    }

    add_helper!(deb_description);
    add_helper!(strip_newlines);
    add_helper!(matches);
}
