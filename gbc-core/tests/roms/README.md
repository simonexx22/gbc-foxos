# Test ROMs

These ROMs are used by the headless conformance tests in `gbc-core/tests/`.
Each ROM reports its result over the emulated serial port.

## Blargg

- `cpu_instrs/` contains the individual Blargg CPU instruction tests from
  <https://github.com/retrio/gb-test-roms/tree/master/cpu_instrs/individual>.
  They are used by `cpu_instrs.rs`; every ROM must print `Passed`.
- `instr_timing/instr_timing.gb` is Blargg's instruction-timing test from
  <https://github.com/retrio/gb-test-roms/tree/master/instr_timing>. It is used
  by `timer.rs` and must print `Passed`.

## Mooneye

- `mooneye/timer/` contains the timer acceptance tests from the
  [Mooneye Test Suite](https://github.com/Gekkio/mooneye-test-suite). They are
  used by `timer.rs`.
- `mooneye/interrupts/` contains the interrupt acceptance tests that do not
  require a full PPU implementation. They are used by `interrupts.rs`.

The timer and interrupt tests use Mooneye's serial result protocol: the final
six bytes must be `03 05 08 0D 15 22` to pass. The suite is distributed under
the MIT license in `mooneye/LICENSE`.

The prebuilt ROMs were obtained from
<https://gekkio.fi/files/mooneye-test-suite/>.
