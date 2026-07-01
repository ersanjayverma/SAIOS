use heapless::Vec;

pub struct ParsedCommand<'a> {
    pub command: &'a str,
    pub args: Vec<&'a str, 16>,
}

pub fn parse_line<'a>(line: &'a str) -> Option<ParsedCommand<'a>> {
    let mut parts = line.split_whitespace();
    let command = parts.next()?;

    let mut args: Vec<&'a str, 16> = Vec::new();
    for part in parts {
        if args.push(part).is_err() {
            break;
        }
    }

    Some(ParsedCommand { command, args })
}
