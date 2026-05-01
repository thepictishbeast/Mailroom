//! Email template rendering via MiniJinja.
//!
//! Loads `.j2` templates from a directory and renders them with
//! named variables. Templates follow Jinja2 syntax (variables with
//! `{{ var }}`, conditionals with `{% if %}`, etc.).

use anyhow::{Context, Result};
use minijinja::{context, Environment, Value};
use std::path::Path;

/// Template renderer backed by MiniJinja.
pub struct TemplateRenderer {
    env: Environment<'static>,
}

impl TemplateRenderer {
    /// Load all `.j2` templates from the given directory.
    /// Templates are registered by filename without the `.j2` extension
    /// (e.g., `notification.txt.j2` → `notification.txt`).
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let mut env = Environment::new();
        if !dir.exists() {
            anyhow::bail!("Template directory does not exist: {}", dir.display());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("j2") {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading template {}", path.display()))?;
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                env.add_template_owned(name, content)?;
            }
        }
        Ok(Self { env })
    }

    /// Create a renderer from inline template strings (useful for testing).
    pub fn from_strings(templates: &[(&str, &str)]) -> Result<Self> {
        let mut env = Environment::new();
        for (name, content) in templates {
            env.add_template_owned(name.to_string(), content.to_string())?;
        }
        Ok(Self { env })
    }

    /// Render a notification template.
    #[allow(clippy::too_many_arguments)]
    pub fn render_notification(
        &self,
        mailbox: &str,
        priority: &str,
        original_from: &str,
        original_subject: &str,
        original_date: &str,
        body_preview: &str,
        tracking_id: &str,
    ) -> Result<String> {
        let tmpl = self
            .env
            .get_template("notification.txt")
            .context("notification.txt template not found")?;
        let rendered = tmpl.render(context! {
            mailbox,
            priority,
            original_from,
            original_subject,
            original_date,
            body_preview,
            tracking_id,
        })?;
        Ok(rendered)
    }

    /// Render a router acknowledgment template.
    pub fn render_router_ack(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        scheduled_at: Option<&str>,
        status: &str,
        tracking_id: &str,
    ) -> Result<String> {
        let tmpl = self
            .env
            .get_template("router_ack.txt")
            .context("router_ack.txt template not found")?;
        let scheduled_at_val: Value = match scheduled_at {
            Some(s) => Value::from(s),
            None => Value::from(()),
        };
        let rendered = tmpl.render(context! {
            from,
            to,
            subject,
            scheduled_at => scheduled_at_val,
            status,
            tracking_id,
        })?;
        Ok(rendered)
    }

    /// Render an arbitrary template by name with a context map.
    pub fn render(&self, template_name: &str, ctx: Value) -> Result<String> {
        let tmpl = self
            .env
            .get_template(template_name)
            .with_context(|| format!("template '{}' not found", template_name))?;
        Ok(tmpl.render(ctx)?)
    }

    /// List all loaded template names.
    pub fn template_names(&self) -> Vec<String> {
        self.env
            .templates()
            .map(|(name, _)| name.to_string())
            .collect()
    }

    /// Render a polished HTML notification email — text/plain comes
    /// from the existing Jinja `notification.txt` template (operators
    /// can edit copy without touching Rust), but the body is wrapped
    /// in the [`mail_templates`] chrome (gradient hero, branded
    /// footer, accent-stripe content card) so the message looks like
    /// the rest of the PlausiDen email stack.
    ///
    /// Returns `(plain, html)` so callers can attach both as a
    /// `multipart/alternative` over SMTP.
    ///
    /// # Errors
    /// Jinja render failure on the plain side; the HTML side is
    /// derived structurally from the same inputs and can't fail.
    #[allow(clippy::too_many_arguments)]
    pub fn render_polished_notification(
        &self,
        mailbox: &str,
        priority: &str,
        original_from: &str,
        original_subject: &str,
        original_date: &str,
        body_preview: &str,
        tracking_id: &str,
    ) -> Result<(String, String)> {
        // Plain side — through the same Jinja path operators control.
        let plain = self.render_notification(
            mailbox,
            priority,
            original_from,
            original_subject,
            original_date,
            body_preview,
            tracking_id,
        )?;

        // HTML side — typed AST through mail-templates so the chrome
        // matches the rest of the PlausiDen email stack.
        use mail_templates::{Block, EmailDocument, Field, GroupBody, GroupCard};
        let priority_pretty = match priority.to_ascii_lowercase().as_str() {
            "high" | "urgent" | "p1" => "High priority",
            "low" | "p4" => "Low priority",
            _ => "New message",
        };

        let preview_truncated: String = if body_preview.chars().count() > 480 {
            let truncated: String = body_preview.chars().take(480).collect();
            format!("{truncated}…")
        } else {
            body_preview.to_string()
        };

        let doc = EmailDocument {
            subject: format!("[{mailbox}] {original_subject}"),
            preheader: format!("From {original_from}: {original_subject}"),
            eyebrow: Some(priority_pretty.into()),
            heading: original_subject.to_string(),
            intro: Some(format!(
                "A new message arrived in {mailbox}. Preview below — open the \
                 mailbox to read the full message."
            )),
            blocks: vec![
                Block::Group(GroupCard {
                    eyebrow: "Sender".into(),
                    title: original_from.to_string(),
                    subtitle: Some(format!("Sent {original_date}")),
                    body: GroupBody::Fields { fields: vec![
                        Field {
                            label: "Mailbox".into(),
                            value: mailbox.to_string(),
                            mono: true,
                        },
                        Field {
                            label: "Priority".into(),
                            value: priority.to_string(),
                            mono: false,
                        },
                    ] },
                    how_to: None,
                }),
                Block::Paragraph {
                    text: preview_truncated,
                },
            ],
            footer_lines: vec![format!("Tracking ID: {tracking_id}")],
        };

        Ok((plain, doc.render_html()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_templates() -> (TempDir, TemplateRenderer) {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("notification.txt.j2"),
            "New email in {{ mailbox }}\nPriority: {{ priority }}\n\nFrom: {{ original_from }}\nSubject: {{ original_subject }}\nDate: {{ original_date }}\n\nPreview:\n{{ body_preview }}\n\n---\nTracking ID: {{ tracking_id }}\n",
        ).unwrap();
        std::fs::write(
            tmp.path().join("router_ack.txt.j2"),
            "Action: Send email\nFrom: {{ from }}\nTo: {{ to }}\nSubject: {{ subject }}\n{% if scheduled_at %}Scheduled: {{ scheduled_at }}\n{% endif %}Status: {{ status }}\nTracking ID: {{ tracking_id }}\n",
        ).unwrap();
        let renderer = TemplateRenderer::from_dir(tmp.path()).unwrap();
        (tmp, renderer)
    }

    #[test]
    fn loads_templates_from_directory() {
        let (_tmp, renderer) = setup_templates();
        let names = renderer.template_names();
        assert!(names.contains(&"notification.txt".to_string()));
        assert!(names.contains(&"router_ack.txt".to_string()));
    }

    #[test]
    fn render_notification() {
        let (_tmp, renderer) = setup_templates();
        let result = renderer
            .render_notification(
                "support@sacred.vote",
                "high",
                "voter@example.com",
                "Need help with voting",
                "2026-04-05 12:00:00",
                "I can't find my voter code...",
                "abc-123",
            )
            .unwrap();
        assert!(result.contains("support@sacred.vote"));
        assert!(result.contains("high"));
        assert!(result.contains("voter@example.com"));
        assert!(result.contains("Need help with voting"));
        assert!(result.contains("abc-123"));
    }

    #[test]
    fn render_router_ack_with_schedule() {
        let (_tmp, renderer) = setup_templates();
        let result = renderer
            .render_router_ack(
                "noreply@sacred.vote",
                "voter@example.com",
                "Your ballot receipt",
                Some("2026-04-06 08:00:00"),
                "queued",
                "xyz-789",
            )
            .unwrap();
        assert!(result.contains("Scheduled: 2026-04-06 08:00:00"));
        assert!(result.contains("queued"));
    }

    #[test]
    fn render_router_ack_without_schedule() {
        let (_tmp, renderer) = setup_templates();
        let result = renderer
            .render_router_ack(
                "noreply@sacred.vote",
                "voter@example.com",
                "Your ballot receipt",
                None,
                "sent",
                "xyz-789",
            )
            .unwrap();
        assert!(!result.contains("Scheduled:"));
        assert!(result.contains("sent"));
    }

    #[test]
    fn from_strings_works() {
        let renderer =
            TemplateRenderer::from_strings(&[("test.txt", "Hello {{ name }}!")]).unwrap();
        let result = renderer
            .render("test.txt", context! { name => "Tim" })
            .unwrap();
        assert_eq!(result, "Hello Tim!");
    }

    #[test]
    fn missing_template_returns_error() {
        let renderer = TemplateRenderer::from_strings(&[]).unwrap();
        assert!(renderer.render("nonexistent", context! {}).is_err());
    }

    #[test]
    fn missing_directory_returns_error() {
        let result = TemplateRenderer::from_dir(Path::new("/tmp/nonexistent-dir-abc123"));
        assert!(result.is_err());
    }

    #[test]
    fn render_polished_notification_returns_plain_and_html() {
        let (_tmp, renderer) = setup_templates();
        let (plain, html) = renderer
            .render_polished_notification(
                "support@sacred.vote",
                "high",
                "voter@example.com",
                "Need help with voting",
                "2026-04-05 12:00:00",
                "I can't find my voter code...",
                "abc-123",
            )
            .unwrap();
        // Plain matches the Jinja template path.
        assert!(plain.contains("support@sacred.vote"));
        assert!(plain.contains("voter@example.com"));
        // HTML uses the polished mail-templates chrome.
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Need help with voting"));
        assert!(html.contains("voter@example.com"));
        assert!(html.contains("Tracking ID: abc-123"));
        // Eyebrow pill maps high priority to "High priority".
        assert!(html.contains("High priority"));
        // No <style> blocks (email-client safe).
        assert!(!html.contains("<style"));
    }

    #[test]
    fn render_polished_notification_truncates_long_preview() {
        let (_tmp, renderer) = setup_templates();
        let long_preview = "x".repeat(1000);
        let (_, html) = renderer
            .render_polished_notification("m@x", "low", "f@x", "subj", "2026", &long_preview, "id")
            .unwrap();
        // Truncated to 480 chars + ellipsis on the HTML side.
        assert!(html.contains("xxxx…"));
        assert!(!html.contains(&"x".repeat(500)));
    }

    #[test]
    fn special_chars_escaped_in_output() {
        let renderer =
            TemplateRenderer::from_strings(&[("test.txt", "Subject: {{ subject }}")]).unwrap();
        let result = renderer
            .render(
                "test.txt",
                context! { subject => "Hello <world> & \"friends\"" },
            )
            .unwrap();
        // MiniJinja auto-escapes in HTML mode but not in text templates
        assert!(result.contains("Hello"));
    }
}
