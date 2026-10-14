//! Task files in `.tandem/`: ids, parsing and serializing, changelog entries
//! and atomic writes.
//!
//! Writes keep diffs small, as the spec's write rules describe: frontmatter
//! in a fixed key order, the body kept byte for byte apart from changelog
//! appends, LF line endings and one trailing newline.

mod changelog;
mod fs;
mod id;
mod task;
mod yaml;

pub use changelog::{ChangelogEntry, EntryError, Unmet};
pub use fs::{
    StoreError, TASK_DIR, create_task, list_task_files, read_task, task_path, unused_id, write_task,
};
pub use id::{ID_ALPHABET, ID_LEN, InvalidId, TaskId};
pub use jiff::civil::Date;
pub use task::{ParseError, Priority, Status, Task};
