use super::session::ShellContext;

pub type ShellResult = Result<(), &'static str>;

pub trait Command {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn execute(&self, ctx: &mut ShellContext, args: &[&str]) -> ShellResult;
}

pub struct StaticCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub handler: fn(&mut ShellContext, &[&str]) -> ShellResult,
}

impl Command for StaticCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn execute(&self, ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
        (self.handler)(ctx, args)
    }
}
