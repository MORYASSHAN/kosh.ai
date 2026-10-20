# kosh-core

The shared vocabulary of Kosh Doctor. Every other crate depends on it.

- `Snapshot`: everything a scan collected, as plain facts. It can be saved as JSON and replayed in tests.
- `Finding`: one problem found by a rule, with evidence, fix steps and a verify step.
- `SafeAction`: the only things Kosh may do for the user (open Settings, a folder or a URL, or copy a command). It is a closed list on purpose: nothing in it changes the computer.
- `anonymize`: removes usernames, computer names and personal folder names before anything is exported or sent anywhere.

This crate makes no system calls.
