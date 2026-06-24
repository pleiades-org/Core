use crate::{
    action_executor::{execute_result_action, ActionError, ExecutedAction},
    command::CommandResult,
    command_router::CommandRouter,
    recent_usage,
    settings::config_directory,
};
use std::{io, path::PathBuf};

pub trait LauncherServices {
    fn search(&self, query: &str) -> Vec<CommandResult>;
    fn execute_result(&self, result: &CommandResult) -> Result<ExecutedAction, ActionError>;
    fn record_usage(&self, result: &CommandResult) -> io::Result<()>;
    fn config_dir(&self) -> PathBuf;
}

pub struct DefaultLauncherServices {
    router: CommandRouter,
}

impl DefaultLauncherServices {
    pub fn new(router: CommandRouter) -> Self {
        Self { router }
    }

    pub fn router(&self) -> &CommandRouter {
        &self.router
    }

    pub fn router_mut(&mut self) -> &mut CommandRouter {
        &mut self.router
    }
}

impl LauncherServices for DefaultLauncherServices {
    fn search(&self, query: &str) -> Vec<CommandResult> {
        self.router.search(query)
    }

    fn execute_result(&self, result: &CommandResult) -> Result<ExecutedAction, ActionError> {
        execute_result_action(result)
    }

    fn record_usage(&self, result: &CommandResult) -> io::Result<()> {
        recent_usage::record_usage(result)
    }

    fn config_dir(&self) -> PathBuf {
        config_directory()
    }
}