// src/bin/admin/main.rs
mod config;
mod kimai_admin;
mod output;
mod setup;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// aw-kimai-admin — manage Kimai customers, projects and activities from the terminal
#[derive(Parser)]
#[command(name = "aw-kimai-admin")]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to config file (defaults to config.toml in current directory)
    #[arg(short, long, default_value = "config.toml", global = true)]
    config: String,

    /// Output format
    #[arg(short = 'f', long, default_value = "table", global = true, value_enum)]
    output: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive first-run wizard: creates a config.toml from scratch
    Setup {
        /// Path to write the generated config file
        #[arg(short = 'p', long, default_value = "config.toml")]
        output_file: String,

        /// Print the generated config to stdout instead of writing a file
        #[arg(long)]
        dry_run: bool,
    },

    /// Manage customers
    Customer {
        #[command(subcommand)]
        action: CustomerAction,
    },
    /// Manage projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Manage activities
    Activity {
        #[command(subcommand)]
        action: ActivityAction,
    },
    /// Scaffold a full team setup: customer + projects + activities in one shot
    Bootstrap {
        #[command(subcommand)]
        action: BootstrapAction,
    },
}

#[derive(Subcommand)]
enum CustomerAction {
    /// List all customers
    List,
    /// Create a new customer
    Create {
        #[arg(help = "Customer name")]
        name: String,
        #[arg(long, help = "Contact email")]
        email: Option<String>,
        #[arg(long, help = "Customer's country")]
        country: Option<String>, 
        #[arg(long, help = "Currency code (e.g. USD, EUR)")]
        currency: Option<String>,
        #[arg(long, help = "Timezone (e.g. America/Chicago)")]
        timezone: Option<String>,
        #[arg(long, help = "Budget in hours")]
        budget: Option<f64>,
    },
    /// Show details for a specific customer
    Get {
        #[arg(help = "Customer ID")]
        id: u32,
    },
    /// Delete a customer
    Delete {
        #[arg(help = "Customer ID")]
        id: u32,
        #[arg(long, help = "Skip confirmation prompt")]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// List all projects (optionally filtered by customer)
    List {
        #[arg(long, help = "Filter by customer ID")]
        customer: Option<u32>,
    },
    /// Create a new project
    Create {
        #[arg(help = "Project name")]
        name: String,
        #[arg(long, help = "Customer ID to attach this project to")]
        customer: u32,
        #[arg(long, help = "Project color (hex, e.g. #4CAF50)")]
        color: Option<String>,
        #[arg(long, help = "Budget in hours")]
        budget: Option<f64>,
        #[arg(long, help = "Start date (YYYY-MM-DD)")]
        start: Option<String>,
        #[arg(long, help = "End date (YYYY-MM-DD)")]
        end: Option<String>,
    },
    /// Show details for a specific project
    Get {
        #[arg(help = "Project ID")]
        id: u32,
    },
    /// Delete a project
    Delete {
        #[arg(help = "Project ID")]
        id: u32,
        #[arg(long, help = "Skip confirmation prompt")]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ActivityAction {
    /// List all activities (optionally filtered by project)
    List {
        #[arg(long, help = "Filter by project ID")]
        project: Option<u32>,
    },
    /// Create a new activity
    Create {
        #[arg(help = "Activity name")]
        name: String,
        #[arg(long, help = "Project ID (omit for a global activity)")]
        project: Option<u32>,
        #[arg(long, help = "Activity color (hex)")]
        color: Option<String>,
        #[arg(long, help = "Budget in hours")]
        budget: Option<f64>,
    },
    /// Show details for a specific activity
    Get {
        #[arg(help = "Activity ID")]
        id: u32,
    },
    /// Delete an activity
    Delete {
        #[arg(help = "Activity ID")]
        id: u32,
        #[arg(long, help = "Skip confirmation prompt")]
        force: bool,
    },
}

#[derive(Subcommand)]
enum BootstrapAction {
    /// Interactively scaffold a new customer with projects and activities
    Run {
        #[arg(long, help = "Load setup from a TOML blueprint file instead of prompting")]
        from_file: Option<String>,
    },
    /// Generate a blank bootstrap blueprint TOML
    InitBlueprint {
        #[arg(default_value = "bootstrap.toml", help = "Output path for the blueprint")]
        output_file: String,
    },
}

#[derive(clap::ValueEnum, Clone)]
pub enum OutputFormat {
    Table,
    Json,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Setup runs before config is loaded (it creates the config) ────────────
    if let Commands::Setup { output_file, dry_run } = &cli.command {
        setup::run(output_file, *dry_run)?;
        return Ok(());
    }

    let config = config::Config::load(&cli.config)?;
    let client = kimai_admin::KimaiAdminClient::new(&config.kimai.url, &config.kimai.token);
    let fmt = cli.output;

    match cli.command {
        // Setup is handled above; this arm is unreachable but required for exhaustiveness.
        Commands::Setup { output_file: _, dry_run: _ } => unreachable!(),

        Commands::Customer { action } => match action {
            CustomerAction::List => {
                let items = client.list_customers().await?;
                output::print_customers(&items, &fmt);
            }
            CustomerAction::Create { name, email, country, currency, timezone, budget } => {
                let req = kimai_admin::CreateCustomerRequest {
                    name,
                    email,
                    country: country.unwrap_or_else(|| "US".to_string()),
                    currency: currency.unwrap_or_else(|| "USD".to_string()),
                    timezone: timezone.unwrap_or_else(|| "America/Chicago".to_string()),
                    budget_type: None,
                    budget,
                    visible: true,
                    color: None,
                };
                let c = client.create_customer(req).await?;
                println!("✓ Customer created  id={} name={:?}", c.id, c.name);
            }
            CustomerAction::Get { id } => {
                let c = client.get_customer(id).await?;
                output::print_customers(&[c], &fmt);
            }
            CustomerAction::Delete { id, force } => {
                confirm_or_abort(force, &format!("Delete customer {id}?"))?;
                client.delete_customer(id).await?;
                println!("✓ Customer {id} deleted");
            }
        },

        Commands::Project { action } => match action {
            ProjectAction::List { customer } => {
                let items = client.list_projects(customer).await?;
                output::print_projects(&items, &fmt);
            }
            ProjectAction::Create { name, customer, color, budget, start, end } => {
                let req = kimai_admin::CreateProjectRequest {
                    name,
                    customer,
                    color,
                    budget,
                    start,
                    end,
                    visible: true,
                    budget_type: None,
                };
                let p = client.create_project(req).await?;
                println!("✓ Project created  id={} name={:?}", p.id, p.name);
            }
            ProjectAction::Get { id } => {
                let p = client.get_project(id).await?;
                output::print_projects(&[p], &fmt);
            }
            ProjectAction::Delete { id, force } => {
                confirm_or_abort(force, &format!("Delete project {id}?"))?;
                client.delete_project(id).await?;
                println!("✓ Project {id} deleted");
            }
        },

        Commands::Activity { action } => match action {
            ActivityAction::List { project } => {
                let items = client.list_activities(project).await?;
                output::print_activities(&items, &fmt);
            }
            ActivityAction::Create { name, project, color, budget } => {
                let req = kimai_admin::CreateActivityRequest {
                    name,
                    project,
                    color,
                    budget,
                    visible: true,
                    budget_type: None,
                };
                let a = client.create_activity(req).await?;
                println!("✓ Activity created  id={} name={:?}", a.id, a.name);
            }
            ActivityAction::Get { id } => {
                let a = client.get_activity(id).await?;
                output::print_activities(&[a], &fmt);
            }
            ActivityAction::Delete { id, force } => {
                confirm_or_abort(force, &format!("Delete activity {id}?"))?;
                client.delete_activity(id).await?;
                println!("✓ Activity {id} deleted");
            }
        },

        Commands::Bootstrap { action } => match action {
            BootstrapAction::Run { from_file } => {
                kimai_admin::bootstrap::run(&client, from_file).await?;
            }
            BootstrapAction::InitBlueprint { output_file } => {
                kimai_admin::bootstrap::write_blank_blueprint(&output_file)?;
                println!("✓ Blueprint written to {output_file}");
                println!("  Edit it, then run: aw-kimai-admin bootstrap run --from-file {output_file}");
            }
        },
    }

    Ok(())
}

fn confirm_or_abort(force: bool, prompt: &str) -> Result<()> {
    if force {
        return Ok(());
    }
    print!("{} [y/N] ", prompt);
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if input.trim().to_lowercase() != "y" {
        anyhow::bail!("Aborted.");
    }
    Ok(())
}
