# Contributing to Airscope

Thanks for your interest. Forks and pull requests are welcome. Airscope is a small hobby project, so keep expectations modest: replies may take a while.

## Reporting a problem or asking for something

Open a GitHub issue. Useful details:

- Your distro, desktop and session type (Wayland or X11), and the Airscope version (shown in the release name).
- Your Bluetooth adapter and Wi-Fi card, if the problem involves them.
- A screenshot. Airscope shows real device names and MAC addresses, so blur anything you don't want public. `AIRSCOPE_DEMO=1` runs it on invented data if you only need to show a visual problem.

Hardware I can't test (a second Bluetooth adapter, other Wi-Fi managers) will only get supported if someone who has it tells me what they see.

## Building

```text
cargo build --release
cargo test
```

The binary is `target/release/airscope`. `packaging/build-appimage.sh` builds the AppImage (needs podman).

Useful switches while developing: `AIRSCOPE_DEMO=1` (or `=bad` to fake a device with problems), `AIRSCOPE_SCREEN=spectrum|devices`, and `AIRSCOPE_SHOT=<file.ppm>` to grab the app's own frame and exit.

## Pull requests

- Open an issue first for anything bigger than a small fix, so we can agree on the approach.
- Keep a change to one topic, and run `cargo test` before sending it.
- Airscope only reads from the system. It shouldn't change adapter or network settings, and it shouldn't need root.
- Using an AI assistant to write your change is fine; the whole project was built that way. What I don't want is automated or unattended AI pull requests (bots, agents opening PRs on their own, bulk drive-by changes). A real person must have read the change, run it, and be ready to answer questions about it. Say in the description if AI wrote it.
- Pull requests that look automated or unreviewed may be closed without comment, and I may restrict who can open them if that becomes a problem.
- By contributing you agree your work is released under the MIT license, like the rest of the project.

## About the code

The whole project was written with Claude Code (see the README). Review it as you would any code you didn't write, and say so in an issue if something looks wrong.
