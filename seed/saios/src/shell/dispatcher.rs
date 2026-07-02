use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::command::ShellResult;
use super::parser::{self, ParsedCommand, RedirectKind};
use super::registry::CommandRegistry;
use super::session::CommandContext;
use crate::console;
use crate::kernel::process;
use crate::saifs;
use crate::vfs;

pub struct CommandDispatcher;

impl CommandDispatcher {
    pub const fn new() -> Self {
        Self
    }

    pub fn dispatch(&self, registry: &CommandRegistry, ctx: &mut CommandContext, line: &str) -> ShellResult {
        let pipelines = parser::parse_line(line);
        for pipeline in pipelines {
            self.dispatch_pipeline(registry, ctx, pipeline.commands)?;
        }
        Ok(())
    }

    fn dispatch_pipeline(
        &self,
        registry: &CommandRegistry,
        ctx: &mut CommandContext,
        mut commands: Vec<ParsedCommand>,
    ) -> ShellResult {
        if commands.is_empty() {
            return Ok(());
        }

        let mut pipe_input: Option<String> = None;
        let len = commands.len();

        for (idx, cmd) in commands.iter_mut().enumerate() {
            self.expand_alias_and_env(ctx, cmd);

            if cmd.command == "." || cmd.command.eq_ignore_ascii_case("source") {
                let path = cmd.args.first().ok_or("source: missing path")?.clone();
                self.run_script(registry, ctx, path.as_str())?;
                continue;
            }

            let mut stdin_data = pipe_input.take();
            let mut out_redirect: Option<(String, bool)> = None;

            for redir in &cmd.redirections {
                match redir.kind {
                    RedirectKind::Read => {
                        stdin_data = Some(saifs::read_text(redir.path.as_str()).map_err(|_| "redirect: input open failed")?);
                    }
                    RedirectKind::Write => {
                        out_redirect = Some((redir.path.clone(), false));
                    }
                    RedirectKind::Append => {
                        out_redirect = Some((redir.path.clone(), true));
                    }
                }
            }

            let suppress_console = idx + 1 < len || out_redirect.is_some();
            let (_exit, captured) = self.execute_command(registry, ctx, cmd, stdin_data.as_deref(), suppress_console)?;

            if let Some((path, append)) = out_redirect {
                self.write_redirect(path.as_str(), captured.as_str(), append)?;
            } else if idx + 1 < len {
                pipe_input = Some(captured);
            }
        }

        Ok(())
    }

    fn execute_command(
        &self,
        registry: &CommandRegistry,
        ctx: &mut CommandContext,
        cmd: &ParsedCommand,
        stdin_data: Option<&str>,
        suppress_console: bool,
    ) -> Result<(i32, String), &'static str> {
        let previous_stdin = ctx.env_get("SISH_STDIN").map(|s| s.to_string());
        if let Some(input) = stdin_data {
            ctx.env_set("SISH_STDIN", input);
        } else {
            ctx.env_unset("SISH_STDIN");
        }

        let argv: Vec<&str> = cmd.args.iter().map(|s| s.as_str()).collect();
        console::begin_output_capture(suppress_console);
        let mut exit_code = 0i32;
        let result = match registry.find(cmd.command.as_str()) {
            Some(command) => {
                let run = command.execute(ctx, argv.as_slice());
                if run.is_err() {
                    exit_code = 1;
                }
                run
            }
            None => {
                match process::exec(cmd.command.as_str(), argv.as_slice(), ctx.session.environment.as_slice()) {
                    Ok(code) => {
                        exit_code = code;
                        ctx.session.last_exit_code = code;
                        if code != 0 && !suppress_console {
                            console::println!("exit {}", code);
                        }
                        Ok(())
                    }
                    Err(_) => {
                        exit_code = 127;
                        if !suppress_console {
                            console::println!("Unknown command: {}", cmd.command);
                        }
                        Ok(())
                    }
                }
            }
        };

        let captured = console::end_output_capture();

        match previous_stdin {
            Some(value) => ctx.env_set("SISH_STDIN", value.as_str()),
            None => ctx.env_unset("SISH_STDIN"),
        }

        result?;
        ctx.session.last_exit_code = exit_code;
        Ok((exit_code, captured))
    }

    fn expand_alias_and_env(&self, ctx: &CommandContext, cmd: &mut ParsedCommand) {
        let mut name = cmd.command.clone();
        let mut alias_args: Vec<String> = Vec::new();

        for _ in 0..8 {
            let Some(alias) = ctx.alias_get(name.as_str()) else {
                break;
            };
            let tokens = alias
                .split_whitespace()
                .map(|s| s.to_string())
                .collect::<Vec<String>>();
            if tokens.is_empty() {
                break;
            }
            name = tokens[0].clone();
            alias_args.extend(tokens.into_iter().skip(1));
        }

        let mut args = alias_args;
        args.extend(cmd.args.iter().cloned());

        cmd.command = self.expand_vars(ctx, name.as_str());
        cmd.args = args
            .into_iter()
            .map(|arg| self.expand_vars(ctx, arg.as_str()))
            .collect();

        for redir in &mut cmd.redirections {
            redir.path = self.expand_vars(ctx, redir.path.as_str());
        }
    }

    fn expand_vars(&self, ctx: &CommandContext, input: &str) -> String {
        let mut out = String::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0usize;

        while i < chars.len() {
            if chars[i] != '$' {
                out.push(chars[i]);
                i += 1;
                continue;
            }

            if i + 1 >= chars.len() {
                out.push('$');
                break;
            }

            if chars[i + 1] == '?' {
                out.push_str(alloc::format!("{}", ctx.session.last_exit_code).as_str());
                i += 2;
                continue;
            }

            let mut j = i + 1;
            let mut key = String::new();

            if chars[j] == '{' {
                j += 1;
                while j < chars.len() && chars[j] != '}' {
                    key.push(chars[j]);
                    j += 1;
                }
                if j < chars.len() && chars[j] == '}' {
                    j += 1;
                }
            } else {
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    key.push(chars[j]);
                    j += 1;
                }
            }

            if key.is_empty() {
                out.push('$');
                i += 1;
                continue;
            }

            if let Some(value) = ctx.env_get(key.as_str()) {
                out.push_str(value);
            }
            i = j;
        }

        out
    }

    fn run_script(&self, registry: &CommandRegistry, ctx: &mut CommandContext, path: &str) -> ShellResult {
        let abs = if path.starts_with('/') {
            path.to_string()
        } else {
            let cwd = saifs::pwd();
            if cwd == "/" {
                alloc::format!("/{}", path)
            } else {
                alloc::format!("{}/{}", cwd, path)
            }
        };

        let script = saifs::read_text(abs.as_str()).map_err(|_| "source: read failed")?;
        for raw in script.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            self.dispatch(registry, ctx, line)?;
        }
        Ok(())
    }

    fn write_redirect(&self, path: &str, data: &str, append: bool) -> ShellResult {
        let final_data = if append {
            let mut merged = saifs::read_text(path).unwrap_or_default();
            merged.push_str(data);
            merged
        } else {
            data.to_string()
        };

        let _ = saifs::touch(path);
        vfs::write_path(path, final_data.as_bytes())
    }
}
