# WKCli
An unofficial CLI app for wanikani.

Its vibecoded with Claude Fable.

This app might mark a correct answer as wrong, break your wk streak, eat your kids (if any) and burn your house. Use at your own causion.

If any of the above happens, do make sure to report a issue on github :)


## Usage

```
wkcli login            # store an API token (needs the "reviews:create" permission)
wkcli                  # do the reviews available now
wkcli status           # reviews / lessons waiting
wkcli --dry-run        # real reviews, nothing submitted
wkcli --demo           # a few built-in items, no account needed
wkcli --limit 20       # at most 20 items
```

During a session: `Enter` submits, `Esc` opens the settings menu (`q` there ends the
session). After an answer is graded: `Enter` next, `Backspace` retype, `f` item info, `p` replay audio.
Reading answers convert romaji to kana as you type; kana from an IME works too.

Settings live in `~/.config/wkcli/config.toml` and can be changed from the menu:

| Setting | Meaning |
| --- | --- |
| `back_to_back` | ask both questions of an item one after the other |
| `question_order` | `reading_first`, `meaning_first` or `random` |
| `lightning` | jump to the next question after a correct answer |
| `retype` | Backspace discards a graded answer so it can be typed again |
| `shake_near_miss` | shake instead of marking wrong on likely slips |
| `wrong_delay_ms` | pause before a wrong answer can be dismissed |
| `audio` | play the pronunciation (random voice) after a vocabulary reading |
| `image_protocol` | `auto`, `kitty`, `sixel`, `iterm2` or `halfblocks` |
| `big_text` | draw kanji and vocabulary as large images (needs terminal graphics) |
| `font` | font family for big text, default `Noto Sans CJK JP` |

The token can also be given through `WANIKANI_API_TOKEN`.

MIT License
