use liquid::Context;

/// Reformat string as per Debian "description" format.
fn deb_description(input: &str) -> String {
    let mut ret = String::new();
    for line in input.lines() {
        match line.trim_right() {
            "" => {
                ret.push_str(" .\n");
            },
            x => {
                ret.push(' ');
                ret.push_str(x);
                ret.push('\n');
            },
        }
    }
    ret.trim_right_matches('\n').to_string()
}

/// Strip newline characters.
fn strip_newlines(input: &str) -> String {
    input.replace("\n", " ")
}

pub fn add_filters<'a>(ctx: &mut Context<'a>) {
    macro_rules! add_filter {
        ($e:expr) => (ctx.filters.insert(stringify!($e).to_owned(), Box::new($e)))
    }

    add_filter!(deb_description);
    add_filter!(strip_newlines);
}
