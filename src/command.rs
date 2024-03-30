use std::process::Stdio;

use tokio::process::Command;

use crate::scrape_target::{FutureScrapeResult, ScrapeOk, ScrapeService};

pub struct CommandScrapeService<T> {
    command_constr: T,
}

impl<T> CommandScrapeService<T>
where
    T: Fn() -> Command + 'static,
{
    pub fn new(command_constr: T) -> Self {
        Self { command_constr }
    }
}

impl<T> ScrapeService for CommandScrapeService<T>
where
    T: Fn() -> Command + 'static,
{
    type Response = ScrapeOk;
    fn call(&mut self) -> FutureScrapeResult<ScrapeOk> {
        let mut command = (self.command_constr)();
        command.kill_on_drop(true);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        Box::pin(async move {
            let child = command.spawn()?;
            let output = child.wait_with_output().await?;
            Ok(ScrapeOk::CommandResponse(output))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn simple_command_execution() {
        let mut cmd_s = CommandScrapeService::new(echo);

        let expected_string = "test";
        let ScrapeOk::CommandResponse(output) = cmd_s.call().await.unwrap() else {
            panic!("Invalid response")
        };
        assert!(output
            .stdout
            .windows(expected_string.len())
            .any(|w| w == expected_string.as_bytes()));
    }

    fn echo() -> Command {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        cmd
    }
}
