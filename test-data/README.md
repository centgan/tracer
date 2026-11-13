# test-data

Real Office files used to prove the storage engine against genuine archives.

Drop a `.pptx`, `.docx` and `.xlsx` in here. Anything in this folder is treated
as a fixture — use throwaway files, not real work.

A synthetic zip built in a unit test cannot stand in for these. Only a real
Office file has the mixed Stored/Deflated entries, the zeroed 1980-01-01
timestamps, and the hundred-plus parts that a genuine deck contains, which is
exactly the shape the engine has to survive.

## Running the check

```
cd src-tauri
cargo test --lib real_office_file_round_trips -- --ignored --nocapture
```

It round-trips every Office file in this folder through `engine::snapshot` and
`engine::restore`, asserting entry order and every part's bytes are preserved,
then prints the path of each rebuilt file.

**Then open one of those rebuilt files in PowerPoint, Word or Excel.** No
assertion here can prove the absence of a repair dialog; that needs a human
and the real application. It is the one check nothing else substitutes for.

The test is `#[ignore]`d so a normal `cargo test` stays fast and offline.
