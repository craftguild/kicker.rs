use anyhow::{Result, ensure};
use dialoguer::{Confirm, Input, Password, Select};
use std::io::IsTerminal;

pub trait SetupUi {
    fn secret(&mut self, prompt: &str) -> Result<String>;
    fn text(&mut self, prompt: &str) -> Result<String>;
    fn header(&mut self) -> Result<Option<(String, String)>>;
}

pub struct TerminalUi;

impl TerminalUi {
    pub fn check_terminal() -> Result<()> {
        ensure!(
            std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
            "new requires an interactive terminal"
        );
        Ok(())
    }
}

impl SetupUi for TerminalUi {
    fn secret(&mut self, prompt: &str) -> Result<String> {
        let choice = Select::new()
            .with_prompt(prompt)
            .items(["Generate a new secret", "Enter an existing secret"])
            .default(0)
            .interact()?;
        if choice == 0 {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes)
                .map_err(|_| anyhow::anyhow!("could not generate a secret"))?;
            let secret = hex::encode(bytes);
            eprintln!("Copy this secret to the webhook service (shown once):\n{secret}");
            ensure!(
                Confirm::new()
                    .with_prompt("Secret copied?")
                    .default(true)
                    .interact()?,
                "setup cancelled"
            );
            Ok(secret)
        } else {
            Password::new()
                .with_prompt("Secret")
                .with_confirmation("Repeat secret", "Secrets do not match")
                .interact()
                .map_err(Into::into)
        }
    }

    fn text(&mut self, prompt: &str) -> Result<String> {
        Input::<String>::new()
            .with_prompt(prompt)
            .interact_text()
            .map_err(Into::into)
    }

    fn header(&mut self) -> Result<Option<(String, String)>> {
        if !Confirm::new()
            .with_prompt("Add an HTTP header?")
            .default(false)
            .interact()?
        {
            return Ok(None);
        }
        let name = Input::<String>::new()
            .with_prompt("Header name")
            .interact_text()?;
        let value = Password::new()
            .with_prompt("Header value (hidden)")
            .allow_empty_password(true)
            .interact()?;
        Ok(Some((name, value)))
    }
}
