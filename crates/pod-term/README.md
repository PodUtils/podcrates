# pod-term

A line discipline in user space, for a host with no pty, and the grammar
of a terminal's output with its ring. It reads and writes bytes only: it
dials nothing and spawns nothing. It has no pod dependency.

## API

| Module | What |
| --- | --- |
| `echo` | `Discipline`: echo, editing, history, a prompt, signals as `Sig` events, for a session with no pty and no discipline below (the one of the SSH server of podvm) |
| `session` | `Session` and `Mode`: which discipline a session gets, by what is below it |
| `passthrough` | the mode for a full-screen program: bytes pass, and the discipline keeps out |
| `termios` | `Tty`, `Termios`, `When`, `Event`, `Signal`, `flags`: the mode that follows the termios state of its child (`TCSETS`, `TCSETSW`, `TCSETSF`), with the rules of Linux `n_tty`: canonical editing, echo, signals, flow control, the mapping of input and output, and SIGWINCH; no prompt and no history |
| `escape`, `refusal` | the escape parser; the escapes that the discipline refuses, with a bell |
| `window` | `Window` and `Size`: the window changes of the session |
| `grammar` | where a stream of output may start, and `grammar::ring`: the last bytes of a shell's output, given from a boundary |

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
| The termios discipline (T-539, for `podssh serve` with no `/dev/ptmx`) | new here: 55 tests (`termios`, `termios_modes`); the plants `term-termios-*` (canonical editing, echo, signals, input mapping, output mapping) |

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_terminal::*` | `pod_term::*` |
| podssh | the test switches `PODSSH_PLANT`, `PODSSH_TEST_TMUX` of the terminal's tests | `POD_TERM_PLANT`, `POD_TERM_TEST_TMUX` |
| podssh | the tty of `podssh serve` with no `/dev/ptmx` (its T-776, T-111) | `pod_term::termios::Tty`, which reads the state that serve answers to `TCGETS` |
| podvm | its own discipline of its SSH server | `pod_term::echo::Discipline` (the same rules) |
