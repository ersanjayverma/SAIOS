use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectKind {
    Read,
    Write,
    Append,
}

#[derive(Clone, Debug)]
pub struct Redirection {
    pub kind: RedirectKind,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct ParsedCommand {
    pub command: String,
    pub args: Vec<String>,
    pub redirections: Vec<Redirection>,
}

#[derive(Clone, Debug)]
pub struct ParsedPipeline {
    pub commands: Vec<ParsedCommand>,
}

fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => {
                let part = line[start..idx].trim();
                if !part.is_empty() {
                    out.push(part);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = line[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }

    out
}

fn split_pipeline(statement: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in statement.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '|' if !in_single && !in_double => {
                let part = statement[start..idx].trim();
                if !part.is_empty() {
                    out.push(part);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = statement[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }

    out
}

fn tokenize(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = part.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
            }
            '>' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
                if chars.peek() == Some(&'>') {
                    let _ = chars.next();
                    out.push(">>".into());
                } else {
                    out.push(">".into());
                }
            }
            '<' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                }
                out.push("<".into());
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

fn parse_command(part: &str) -> Option<ParsedCommand> {
    let tokens = tokenize(part);
    if tokens.is_empty() {
        return None;
    }

    let mut command = None;
    let mut args = Vec::new();
    let mut redirections = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        match token.as_str() {
            ">" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Write,
                    path,
                });
                i += 2;
                continue;
            }
            ">>" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Append,
                    path,
                });
                i += 2;
                continue;
            }
            "<" => {
                let path = tokens.get(i + 1)?.clone();
                redirections.push(Redirection {
                    kind: RedirectKind::Read,
                    path,
                });
                i += 2;
                continue;
            }
            _ => {}
        }

        if command.is_none() {
            command = Some(token.clone());
        } else {
            args.push(token.clone());
        }

        i += 1;
    }

    Some(ParsedCommand {
        command: command?,
        args,
        redirections,
    })
}

pub fn parse_line(line: &str) -> Vec<ParsedPipeline> {
    let mut pipelines = Vec::new();
    for statement in split_statements(line) {
        let mut commands = Vec::new();
        for part in split_pipeline(statement) {
            if let Some(cmd) = parse_command(part) {
                commands.push(cmd);
            }
        }
        if !commands.is_empty() {
            pipelines.push(ParsedPipeline { commands });
        }
    }
    pipelines
}
