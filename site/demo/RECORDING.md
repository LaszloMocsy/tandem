# Recording the hero clip

The landing page's hero is an 18-second loop of the real app: an agent picks up a task, works it and finishes it, and you review it. This kit makes every take the same.

- `seed.ts` builds the demo repo, a small CLI called `pocket-weather`, at `/tmp/tandem-demo`. Its tasks are written by Tandem's own service (`core/examples/demo_seed.rs`), not by hand. Rerun it to reset the board between takes.
- `drive.ts` calls Tandem's MCP tools on a fixed timeline, as Codex and Claude Code.

## The board after seeding

| Column      | Tasks                                                                                            |
| ----------- | ------------------------------------------------------------------------------------------------ |
| Backlog     | Cache forecasts for 10 minutes · Show sunrise and sunset times (both proposed by agents)         |
| Todo        | **Add a `--units` flag** (high, 3 criteria) · Read the API key from the keychain                 |
| In progress | Retry failed requests with backoff, held by `codex-…` (claimed by `drive.ts`)                    |
| Review      | Parse city names with spaces                                                                     |
| Done        | Set up the project and CI · Fetch current conditions · Print a one-line summary                  |

Right after `seed.ts`, the retry task is still in Todo; `drive.ts` claims it as Codex before the timeline starts, so its lease is live on camera. Every task has real commits: the code with the task file in progress, the finished task file, and for Done tasks the review commit.

## Setup, once

1. Build a release app, so there's no DEV strip: `cargo tauri build`, then open the bundle under `target/release/bundle/`. Quit any other Tandem first; only one app can serve MCP on port 4317.
2. `bun site/demo/seed.ts`
3. In Tandem, open `/tmp/tandem-demo` from the project switcher (**Open folder…**).
4. Open the **Setup** panel and copy the project's MCP URL, such as `http://127.0.0.1:4317/p/tandem-demo/mcp`.

## Each take

1. `bun site/demo/seed.ts` to reset the repo. Tandem picks up the new files; if the board looks wrong, use **Settings → Project → Reset local state**.
2. `bun site/demo/drive.ts <mcp-url>`. It claims the retry task as Codex and waits.
3. Start recording, hold the board at rest for a moment, then press Enter in the terminal. That moment is 0s below.
4. Follow the shot list. Stop recording, then Ctrl-C `drive.ts`.

## Shot list: the 18s hero loop

| Time   | What happens                                                                                                  | Who      |
| ------ | ------------------------------------------------------------------------------------------------------------- | -------- |
| 0–2s   | Board at rest. Codex's card in In progress shows its live badge.                                              | —        |
| 2s     | `start_task`: **Add a `--units` flag** moves from Todo to In progress with Claude Code's live badge.          | drive.ts |
| 4.5s   | `log_progress`: "Parsing --units with a small enum"                                                           | drive.ts |
| 6.5s   | `log_progress`: "Locale default done, adding tests"                                                           | drive.ts |
| 8s     | `finish_task`, all three criteria met: the card moves to Review.                                              | drive.ts |
| ~9.5s  | You click the card. The detail pane shows the summary and the ticked criteria.                                | you      |
| ~12s   | You press **R**: the card moves to Done.                                                                      | you      |
| 12–17s | Hold on the board, cursor still.                                                                              | —        |
| 17–18s | Fade back to the first frame (done in the encode below), so the loop is seamless.                             | —        |

The timeline lives in `TIMELINE` at the top of `drive.ts`. If you change it, change this table too.

## Optional clips

Record these only if you want them for How it works; otherwise the page keeps its HTML mocks.

- **A: the task file diff.** After a take, show `git diff` of the `--units` task file in a terminal or editor: `status: todo` → `status: in_progress`, then the finished changelog entry.
- **B: Setup panel copy.** Open the Setup panel and click the copy button for the Claude Code command.
- **C: an agent proposes, you approve.** Ask an agent connected to the demo project to propose a child task of **Add a `--units` flag** with `create_task` and `parent`. It lands in Backlog; select it and press **A** to approve it into Todo.

## Checklist

- [ ] Release build (no DEV, ALPHA or BETA strip), only one Tandem running
- [ ] Window exactly 1440×900. On macOS:
      `osascript -e 'tell application "System Events" to tell process "Tandem" to set size of window 1 to {1440, 900}'`
- [ ] Light take first, then a dark take (Settings → Theme, or the system appearance)
- [ ] Clean desktop: no notifications (Do Not Disturb on), hidden dock, no other windows behind Tandem
- [ ] Slow, deliberate cursor; no hovering over unrelated controls
- [ ] 60fps capture (macOS: Cmd-Shift-5, record the selected window or a portion, Options → Show Mouse Clicks off)
- [ ] Crop to the window in the encode, not by hand
- [ ] Hand over the raw files as `raw-light.mov` and `raw-dark.mov`

## Encoding

Not run on this machine (ffmpeg wasn't installed when the kit was written), so check the crop, sizes and timings on the first real take. You need an ffmpeg with `libsvtav1` and `libx264` (`brew install ffmpeg`).

Set these per take: `START` is where 0s is in the raw file, and `CROP` is the window's `width:height:x:y` in the raw file's pixels (a Retina capture is 2880×1800 for a 1440×900 window).

```sh
TAKE=light            # or dark
START=00:00:03.20     # 0s on the shot list
CROP=2880:1800:0:0    # the window, in raw pixels

# 1. A lossless 18s master. The last second crossfades into the first, so
#    the loop has no jump.
ffmpeg -ss "$START" -t 19 -i "raw-$TAKE.mov" -filter_complex \
  "[0:v]crop=$CROP,scale=1440:900:flags=lanczos,fps=30,split[main][head]; \
   [head]trim=0:1,setpts=PTS-STARTPTS[first]; \
   [main]trim=0:18,setpts=PTS-STARTPTS[body]; \
   [body][first]xfade=transition=fade:duration=1:offset=17,format=yuv420p[v]" \
  -map "[v]" -an -c:v libx264 -crf 0 -preset veryfast "master-$TAKE.mkv"

# 2. AV1 WebM, aim for 1.5 MB or less: raise -crf if it's bigger.
ffmpeg -i "master-$TAKE.mkv" -c:v libsvtav1 -crf 40 -preset 4 -g 300 \
  -pix_fmt yuv420p -an "hero-$TAKE.webm"

# 3. H.264 MP4 for browsers without AV1, same 1.5 MB budget.
ffmpeg -i "master-$TAKE.mkv" -c:v libx264 -crf 30 -preset veryslow -tune animation \
  -pix_fmt yuv420p -movflags +faststart -an "hero-$TAKE.mp4"

# 4. Poster: the frame at 3s, as PNG; the site converts it to WebP.
ffmpeg -ss 3 -i "master-$TAKE.mkv" -frames:v 1 "hero-$TAKE-poster.png"

ls -lh "hero-$TAKE".*
```

The encode is 30fps to keep the files small; the 60fps capture still gives smooth frames to pick from. If the board's motion looks choppy, try `fps=60` in step 1 and raise the CRFs to stay in budget.
