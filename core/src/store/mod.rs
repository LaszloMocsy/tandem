//! Task files in `.tandem/`: ids, parsing and serializing, changelog entries,
//! atomic writes, and the folder's `config.toml` and `README.md`.
//!
//! Writes keep diffs small, as the spec's write rules describe: frontmatter
//! in a fixed key order, the body kept byte for byte apart from changelog
//! appends, LF line endings and one trailing newline.

mod changelog;
mod folder;
mod fs;
mod id;
mod markdown;
mod sections;
mod task;
mod yaml;

pub use changelog::{ChangelogEntry, EntryError, EntryHeading, Unmet};
pub use folder::{Access, CONFIG_FILE, FORMAT_VERSION, README_FILE, ReadOnly, prepare_folder};
pub use fs::{
    StoreError, TASK_DIR, create_task, list_task_files, parse_task_file, read_task, task_path,
    unused_id, write_task,
};
pub use id::{ID_ALPHABET, ID_LEN, InvalidId, TaskId};
pub use jiff::civil::Date;
pub use sections::{
    ACCEPTANCE_CRITERIA, CHANGELOG, Criterion, DESCRIPTION, RELEVANT_FILES, SectionError,
};
pub use task::{ParseError, Priority, Status, Task, UnknownValue};
