//! Output formatting and display.
//!
//! Provides flexible output formatting for CLI commands.

use comfy_table::{presets, Cell, Color, ContentArrangement, Table};
use console::{style, Style};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::fmt::Display;
use std::io::{self, Write};
use std::time::Duration;

/// Output format options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable table format.
    #[default]
    Table,
    /// JSON format.
    Json,
    /// YAML format.
    Yaml,
    /// Compact single-line format.
    Compact,
}

impl OutputFormat {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "table" => Some(Self::Table),
            "json" => Some(Self::Json),
            "yaml" => Some(Self::Yaml),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

/// Output writer with configurable format.
pub struct OutputWriter {
    format: OutputFormat,
    colors: bool,
    multi_progress: MultiProgress,
}

impl OutputWriter {
    /// Create a new output writer.
    pub fn new(format: OutputFormat, colors: bool) -> Self {
        Self {
            format,
            colors,
            multi_progress: MultiProgress::new(),
        }
    }

    /// Get the output format.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Check if colors are enabled.
    pub fn colors_enabled(&self) -> bool {
        self.colors
    }

    /// Write a success message.
    pub fn success(&self, msg: impl Display) {
        if self.colors {
            println!("{} {}", style("✓").green().bold(), msg);
        } else {
            println!("[OK] {}", msg);
        }
    }

    /// Write an error message.
    pub fn error(&self, msg: impl Display) {
        if self.colors {
            eprintln!("{} {}", style("✗").red().bold(), msg);
        } else {
            eprintln!("[ERROR] {}", msg);
        }
    }

    /// Write a warning message.
    pub fn warning(&self, msg: impl Display) {
        if self.colors {
            println!("{} {}", style("⚠").yellow().bold(), msg);
        } else {
            println!("[WARN] {}", msg);
        }
    }

    /// Write an info message.
    pub fn info(&self, msg: impl Display) {
        if self.colors {
            println!("{} {}", style("ℹ").blue().bold(), msg);
        } else {
            println!("[INFO] {}", msg);
        }
    }

    /// Write a header.
    pub fn header(&self, msg: impl Display) {
        if self.colors {
            println!("\n{}", style(msg.to_string()).cyan().bold());
            println!("{}", style("─".repeat(40)).dim());
        } else {
            println!("\n=== {} ===", msg);
        }
    }

    /// Write a key-value pair.
    pub fn kv(&self, key: impl Display, value: impl Display) {
        if self.colors {
            println!("  {}: {}", style(key.to_string()).dim(), value);
        } else {
            println!("  {}: {}", key, value);
        }
    }

    /// Write data in the configured format.
    pub fn write<T: Serialize>(&self, data: &T) -> io::Result<()> {
        match self.format {
            OutputFormat::Json => {
                let output = serde_json::to_string_pretty(data)?;
                println!("{}", output);
            }
            OutputFormat::Yaml => {
                let output = serde_yaml::to_string(data)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                println!("{}", output);
            }
            OutputFormat::Compact => {
                let output = serde_json::to_string(data)?;
                println!("{}", output);
            }
            OutputFormat::Table => {
                // For Table format, caller should use write_table
                let output = serde_json::to_string_pretty(data)?;
                println!("{}", output);
            }
        }
        Ok(())
    }

    /// Create a new progress bar.
    pub fn progress(&self, total: u64, msg: impl Into<String>) -> ProgressBar {
        let pb = self.multi_progress.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::with_template(if self.colors {
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}"
            } else {
                "[{elapsed_precise}] [{bar:40}] {pos}/{len} {msg}"
            })
            .unwrap()
            .progress_chars("█▓░"),
        );
        pb.set_message(msg.into());
        pb
    }

    /// Create a spinner.
    pub fn spinner(&self, msg: impl Into<String>) -> ProgressBar {
        let pb = self.multi_progress.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::with_template(if self.colors {
                "{spinner:.green} {msg}"
            } else {
                "[*] {msg}"
            })
            .unwrap(),
        );
        pb.set_message(msg.into());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    /// Get the multi-progress instance.
    pub fn multi_progress(&self) -> &MultiProgress {
        &self.multi_progress
    }
}

/// Table builder for structured output.
pub struct TableBuilder {
    table: Table,
    colors: bool,
}

impl TableBuilder {
    /// Create a new table builder.
    pub fn new(colors: bool) -> Self {
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL_CONDENSED);
        table.set_content_arrangement(ContentArrangement::Dynamic);

        Self { table, colors }
    }

    /// Set the table header.
    pub fn header(mut self, columns: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let cells: Vec<Cell> = columns
            .into_iter()
            .map(|c| {
                if self.colors {
                    Cell::new(c.as_ref()).fg(Color::Cyan)
                } else {
                    Cell::new(c.as_ref())
                }
            })
            .collect();
        self.table.set_header(cells);
        self
    }

    /// Add a row to the table.
    pub fn row(mut self, values: impl IntoIterator<Item = impl Display>) -> Self {
        let cells: Vec<Cell> = values.into_iter().map(|v| Cell::new(v.to_string())).collect();
        self.table.add_row(cells);
        self
    }

    /// Add a row with colored status.
    pub fn status_row(
        mut self,
        values: impl IntoIterator<Item = impl Display>,
        status: StatusType,
    ) -> Self {
        let values: Vec<String> = values.into_iter().map(|v| v.to_string()).collect();
        let mut cells: Vec<Cell> = values.iter().map(|v| Cell::new(v)).collect();

        if self.colors && !cells.is_empty() {
            let color = match status {
                StatusType::Success => Color::Green,
                StatusType::Warning => Color::Yellow,
                StatusType::Error => Color::Red,
                StatusType::Info => Color::Blue,
                StatusType::Neutral => Color::White,
            };
            // Color the last cell (usually the status)
            if let Some(last) = cells.last_mut() {
                *last = Cell::new(&values[values.len() - 1]).fg(color);
            }
        }
        self.table.add_row(cells);
        self
    }

    /// Build and return the table.
    pub fn build(self) -> Table {
        self.table
    }

    /// Print the table.
    pub fn print(self) {
        println!("{}", self.table);
    }
}

/// Status type for colored output.
#[derive(Debug, Clone, Copy)]
pub enum StatusType {
    Success,
    Warning,
    Error,
    Info,
    Neutral,
}

/// Protocol status display.
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolStatus {
    pub protocol: String,
    pub devices: usize,
    pub points: usize,
    pub status: String,
    pub uptime: String,
}

/// Device summary for list output.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub status: String,
    pub points: usize,
    pub last_update: String,
}

/// Validation result for output.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub path: String,
    pub message: String,
}
