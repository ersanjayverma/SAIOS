use alloc::format;
use alloc::string::String;

use super::session::ShellSession;

pub trait PromptProvider {
    fn render(&self) -> String;
}

pub struct SessionPromptProvider<'a> {
    session: &'a ShellSession,
}

impl<'a> SessionPromptProvider<'a> {
    pub fn new(session: &'a ShellSession) -> Self {
        Self { session }
    }
}

impl PromptProvider for SessionPromptProvider<'_> {
    fn render(&self) -> String {
        if let Some(user) = &self.session.current_user {
            format!("{}@SAIOS:{}>", user, self.session.current_working_directory)
        } else {
            format!("SAIOS:{}>", self.session.current_working_directory)
        }
    }
}
