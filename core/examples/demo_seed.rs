//! Seeds the landing page's demo repo through Tandem's own service, so every
//! task file is written the way the app writes it. Run by
//! `site/demo/seed.ts`, which first creates the repo and its first commit:
//!
//! ```sh
//! cargo run -p tandem-board-core --example demo_seed -- /tmp/tandem-demo
//! ```
//!
//! It works in a throwaway index, so the app's own database is untouched;
//! the app rebuilds its board from the files when you open the repo. Leaves
//! the board as `site/demo/RECORDING.md` describes, except for the task in
//! progress, which `drive.ts` claims through MCP so its lease is live.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tandem_board_core::service::{Actor, Agent, Config, Finish, NewTask, Service, Written};
use tandem_board_core::store::{Priority, TaskId};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// The name changelog entries use for you.
const USER_NAME: &str = "Sam Lee";

fn main() -> Result<()> {
    let repo: PathBuf = std::env::args_os()
        .nth(1)
        .ok_or("usage: demo_seed <repo>")?
        .into();
    if !repo.join(".git").is_dir() {
        return Err(format!(
            "{} is not a git repo; run site/demo/seed.ts",
            repo.display()
        )
        .into());
    }

    let data = tempfile::TempDir::new()?;
    let config = Config {
        user_name: USER_NAME.to_owned(),
        ..Config::default()
    };
    let service = Service::open(data.path(), "demo", &repo, config)?;
    let seed = Seed {
        service: &service,
        repo: &repo,
        claude: agent(&service, "claude-code")?,
        codex: agent(&service, "codex")?,
    };
    seed.run()
}

fn agent(service: &Service, client: &str) -> Result<Actor> {
    Ok(Actor::Agent(Agent {
        id: service.register_agent(Some(client))?,
        client_name: Some(client.to_owned()),
    }))
}

struct Seed<'a> {
    service: &'a Service,
    repo: &'a Path,
    claude: Actor,
    codex: Actor,
}

/// A task you write, as on the board's New task form.
struct Spec<'a> {
    title: &'a str,
    description: &'a str,
    criteria: &'a [&'a str],
    priority: Priority,
    labels: &'a [&'a str],
}

/// The code an agent commits for a task.
struct Work<'a> {
    commit: &'a str,
    files: &'a [(&'a str, &'a str)],
    summary: &'a str,
}

impl Seed<'_> {
    fn run(&self) -> Result<()> {
        // Opening the service created .tandem/ with its config and README.
        self.commit(
            "chore: track tasks with Tandem",
            &[
                ".tandem/config.toml".to_owned(),
                ".tandem/README.md".to_owned(),
            ],
        )?;

        // Done: worked, committed, reviewed and the review committed.
        for (spec, agent, work) in [
            (
                Spec {
                    title: "Set up the project and CI",
                    description: "TypeScript, Bun and a CI workflow that runs the tests.",
                    criteria: &["`bun test` runs in CI on every push"],
                    priority: Priority::High,
                    labels: &["infra"],
                },
                &self.codex,
                Work {
                    commit: "ci: run the tests on every push",
                    files: &[(".github/workflows/ci.yml", CI_YML)],
                    summary: "Added a CI workflow that installs Bun and runs the tests.",
                },
            ),
            (
                Spec {
                    title: "Fetch current conditions",
                    description: "Call the weather API for a city and return temperature and conditions.",
                    criteria: &[
                        "`fetchConditions(city)` returns temperature and a description",
                        "Network errors surface as a readable message",
                    ],
                    priority: Priority::High,
                    labels: &["network"],
                },
                &self.claude,
                Work {
                    commit: "feat: fetch current conditions",
                    files: &[("src/api.ts", API_TS)],
                    summary: "Added fetchConditions with a typed result and readable errors.",
                },
            ),
            (
                Spec {
                    title: "Print a one-line summary",
                    description: "Print the conditions as one line, such as `Lisbon 21°C, clear`.",
                    criteria: &["The CLI prints one line per city"],
                    priority: Priority::Medium,
                    labels: &["cli"],
                },
                &self.claude,
                Work {
                    commit: "feat: print a one-line summary",
                    files: &[("src/format.ts", FORMAT_TS)],
                    summary: "Added formatSummary and used it in the CLI.",
                },
            ),
        ] {
            let id = self.create(&spec)?;
            self.work(id, agent, &work)?;
            self.service.mark_reviewed(id)?;
            self.service.commit_review(id)?;
        }

        // Review: finished and committed, waiting for you.
        let id = self.create(&Spec {
            title: "Parse city names with spaces",
            description: "`pocket-weather New York` should look up one city, not two.",
            criteria: &[
                "Words after the command form one city name",
                "Quoted names still work",
            ],
            priority: Priority::Medium,
            labels: &["cli"],
        })?;
        self.work(
            id,
            &self.codex,
            &Work {
                commit: "fix: treat the arguments as one city name",
                files: &[("src/args.ts", ARGS_TS)],
                summary: "Joined the arguments into one city name; quoted names pass through unchanged.",
            },
        )?;

        // Todo. The hero clip's agent works --units; drive.ts claims the
        // retry task as Codex before recording, so it shows in progress.
        self.create(&Spec {
            title: "Add a --units flag",
            description: "Let people pick metric or imperial output.",
            criteria: &[
                "`--units` accepts metric and imperial",
                "The default follows the system locale",
                "`--help` documents the flag",
            ],
            priority: Priority::High,
            labels: &["cli"],
        })?;
        self.create(&Spec {
            title: "Retry failed requests with backoff",
            description: "Retry timeouts and 5xx responses up to three times, backing off between tries.",
            criteria: &["Retries at most three times", "Gives up with the last error"],
            priority: Priority::High,
            labels: &["network"],
        })?;
        self.create(&Spec {
            title: "Read the API key from the keychain",
            description: "Stop reading the key from an environment variable.",
            criteria: &["The key is stored in and read from the OS keychain"],
            priority: Priority::Medium,
            labels: &["security"],
        })?;

        // Backlog: proposed by agents, waiting for your approval.
        for (agent, title, labels, priority) in [
            (
                &self.claude,
                "Cache forecasts for 10 minutes",
                ["cli"],
                Priority::Medium,
            ),
            (
                &self.codex,
                "Show sunrise and sunset times",
                ["ux"],
                Priority::Low,
            ),
        ] {
            self.service.create_task(
                agent,
                NewTask {
                    title: title.to_owned(),
                    priority: Some(priority),
                    labels: labels.map(str::to_owned).to_vec(),
                    ..NewTask::default()
                },
            )?;
        }
        self.commit("chore(tandem): plan the next tasks", &[])
    }

    fn create(&self, spec: &Spec) -> Result<TaskId> {
        let written = self.service.create_task(
            &Actor::User,
            NewTask {
                title: spec.title.to_owned(),
                description: spec.description.to_owned(),
                acceptance_criteria: spec.criteria.iter().map(|c| (*c).to_owned()).collect(),
                priority: Some(spec.priority),
                labels: spec.labels.iter().map(|l| (*l).to_owned()).collect(),
                ..NewTask::default()
            },
        )?;
        Ok(written.id)
    }

    /// Start, write the code, commit it with the task file, finish, and
    /// commit the finished file: the agent workflow from the spec.
    fn work(&self, id: TaskId, agent: &Actor, work: &Work) -> Result<()> {
        let started = self.service.start_task(agent, id)?;
        let mut paths = Vec::new();
        for (path, contents) in work.files {
            let full = self.repo.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(full, contents)?;
            paths.push((*path).to_owned());
        }
        self.commit_task(work.commit, &started, &paths)?;

        let criteria = self
            .service
            .get_task(agent, id)?
            .content
            .map_or(0, |c| c.criteria.len());
        let finished = self.service.finish_task(
            agent,
            id,
            Finish {
                summary: work.summary.to_owned(),
                files_changed: paths,
                met: (1..=u32::try_from(criteria)?).collect(),
                ..Finish::default()
            },
        )?;
        self.commit_task(&format!("chore(tandem): finish {id}"), &finished, &[])
    }

    fn commit_task(&self, message: &str, task: &Written, paths: &[String]) -> Result<()> {
        let mut all = paths.to_vec();
        all.push(task.path.clone());
        self.commit(&format!("{message}\n\nTask: {}", task.id), &all)
    }

    /// Commits `paths`, or every change when it is empty.
    fn commit(&self, message: &str, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            self.git(&["add", "--all"])?;
        } else {
            let mut args = vec!["add", "--"];
            args.extend(paths.iter().map(String::as_str));
            self.git(&args)?;
        }
        self.git(&["commit", "--quiet", "-m", message])
    }

    fn git(&self, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .arg("-C")
            .arg(self.repo)
            .args(args)
            .status()?;
        if !status.success() {
            return Err(format!("git {} failed", args.join(" ")).into());
        }
        Ok(())
    }
}

const CI_YML: &str = "name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v2
      - run: bun install
      - run: bun test
";

const API_TS: &str = "export interface Conditions {
  city: string;
  temperatureC: number;
  description: string;
}

export async function fetchConditions(city: string): Promise<Conditions> {
  const url = `https://api.example.com/v1/current?city=${encodeURIComponent(city)}`;
  let res: Response;
  try {
    res = await fetch(url, { headers: { Authorization: `Bearer ${process.env.WEATHER_KEY}` } });
  } catch {
    throw new Error(`Could not reach the weather service for ${city}.`);
  }
  if (!res.ok) throw new Error(`The weather service answered ${res.status} for ${city}.`);
  const body = (await res.json()) as { temp_c: number; summary: string };
  return { city, temperatureC: body.temp_c, description: body.summary };
}
";

const FORMAT_TS: &str = "import type { Conditions } from './api';

export function formatSummary(c: Conditions): string {
  return `${c.city} ${Math.round(c.temperatureC)}°C, ${c.description}`;
}
";

const ARGS_TS: &str =
    "/** The city from the command-line arguments: every word after the command. */
export function cityFrom(argv: string[]): string {
  return argv.join(' ').trim();
}
";
