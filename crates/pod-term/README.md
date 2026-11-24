# pod-term

A line discipline in user space, for a host with no pty, and the grammar
of a terminal's output with its ring. It reads and writes bytes only: it
dials nothing and spawns nothing. It has no pod dependency. The feature
`transfer` adds base64, CRC-32 (crc32fast) and SHA-256 (sha2), each pure
Rust.

## API

| Module | What |
| --- | --- |
| `echo` | `Discipline`: echo, editing, history, a prompt (`prompt()`; none after `without_prompt()`, for a command that a server runs, also `Session::without_prompt`), signals as `Sig` events, for a session with no pty and no discipline below (the one of the SSH server of podvm) |
| `session` | `Session` and `Mode`: which discipline a session gets, by what is below it |
| `passthrough` | the mode for a full-screen program: bytes pass, and the discipline keeps out |
| `termios` | `Tty`, `Termios`, `When`, `Event`, `Signal`, `flags`: the mode that follows the termios state of its child (`TCSETS`, `TCSETSW`, `TCSETSF`), with the rules of Linux `n_tty`: canonical editing, echo, signals, flow control, the mapping of input and output, and SIGWINCH; no prompt and no history |
| `escape`, `refusal` | the escape parser; the escapes that the discipline refuses, with a bell |
| `window` | `Window` and `Size`: the window changes of the session |
| `grammar` | where a stream of output may start, and `grammar::ring`: the last bytes of a shell's output, given from a boundary; `Ring::modes_at`, the `Modes` that the stream set before an offset that the ring holds, also after it dropped bytes |
| `modes` | `Modes`: the modes that a stream of output set (the cursor keys, origin, autowrap, the cursor, the alternate screen, the mouse level and encoding, focus, bracketed paste, the keypad, G0), and `Modes::restore`, the bytes that put a new terminal in them |
| `transfer` (feature `transfer`) | `Transfer`: a sans-IO 7-bit transfer of one file through a terminal that repaints its output (`#KIND:` frames of base64 with a CRC-32, one frame in flight, the echo of the received count, a chunk of 1 to 32 KiB, a resync at the last `#KIND:`, and the SHA-256 and the rename as events), for a pty that damages ZMODEM. Version 2 of the wire: the receiver answers a frame that comes again while its caller acts (an offer to a person, a rename) with the frame `HOLD`, and the sender counts its 20 s wait from the last frame of any kind, with no limit on the time of the person; the receiver keeps the size and the SHA-256 of the kept file for `LINGER` (25 s) after `KEPT` and answers a `DONE` that comes again with the same `KEPT` (`Transfer::lingers`: the far program reads its input until it is false); a sender with no frame in 20 s after `DONE` ends with `Ended::Unconfirmed` ("sent; the receiver did not confirm"), not with a stop. A peer of version 1 gets `Event::Unsupported` at its trigger, or `STOP` with `stop::UNSUPPORTED` at its `HELO`. The codec is the source of the frame table and the waits; podssh's `docs/terminal.md`, "The 7-bit envelope", follows it |

## Parity

| Capability of podssh `crates/podssh-terminal` | Here |
| --- | --- |
| The echo discipline with editing, history, the line cap and the bell | yes, the same code |
| Passthrough for a full-screen program | yes |
| The refusals of escapes | yes |
| Window changes | yes |
| The grammar of output and its ring | yes |
| The tests: 176 (unit, discipline, keys, motion, oracle, calibration, properties, grammar, window, plants) | yes, with their names |
| The plants C, D and E (`tests/plants.rs`) | yes: `scripts/plants/term-plant-*.plant`; the switch is `POD_TERM_PLANT` |
| The termios discipline (T-539, for `podssh serve` with no `/dev/ptmx`) | new here: 55 tests (`termios`, `termios_modes`), and `termios_kernel` (T-540, Linux): 53 cases of those two files given to `Tty` and to a pty of the kernel (`posix_openpt`, `tcsetattr`), the bytes for the child and the user equal after each step, skipped with its reason where `/dev/ptmx` does not open; the plants `term-termios-*` (canonical editing, echo, signals, input mapping, output mapping, and `term-termios-kernel-onlcr` against the kernel) |

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_terminal::*` | `pod_term::*` |
| podssh | the test switches `PODSSH_PLANT`, `PODSSH_TEST_TMUX` of the terminal's tests | `POD_TERM_PLANT`, `POD_TERM_TEST_TMUX` |
| podssh | the tty of `podssh serve` with no `/dev/ptmx` (PodUtils/podssh@T-776, PodUtils/podssh@T-111) | `pod_term::termios::Tty`, which reads the state that serve answers to `TCGETS` |
| podvm | its own discipline of its SSH server | `pod_term::echo::Discipline` (the same rules) |
| podssh | the draft `modes_before` of `podssh-ssh/src/server/kept_screen.rs` (PodUtils/podssh@T-161) | `Ring::modes_at`, then `pod_term::modes::Modes::restore` |
| podssh | the 7-bit envelope of PodUtils/podssh@T-831, step 2 (not written in podssh) | `pod_term::transfer::Transfer`, with `features = ["transfer"]`; the far program that receives exits when `active()` and `lingers()` are both false, and the caller words `Ended::Unconfirmed` as sent with no confirmation |
