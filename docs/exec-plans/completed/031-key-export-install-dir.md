# ExecPlan 031: Default Key Export to Install Directory

## Goal
When the encrypted API-key export path is empty, save the file beside the running Loadout daemon executable.

## Context
The current key export defaults to `<data dir>\\backups`; users may also provide an explicit destination. This change applies only to encrypted key exports. Configuration and database backups keep their existing defaults.

## Existing behavior
`keys_export::export` delegates empty paths to `backup::target_path(data_dir, ...)`.

## Constraints
- Preserve explicit output paths.
- Keep key exports encrypted and require existing confirmation.
- Do not change ordinary backup locations.

## Architecture impact
Pass the daemon executable directory through `DaemonState` to key export handling. If the executable path cannot be resolved, fall back to the data directory for a writable predictable destination.

## Files expected to change
- `crates/core/src/daemon.rs`
- `crates/core/src/dispatcher.rs`
- `crates/core/src/keys_export.rs`
- `crates/ipc/src/api.rs`
- `apps/desktop/src/i18n.tsx`

## Implementation stages
1. Capture the daemon executable directory at startup and pass it through state.
2. Use it only as the default directory for key exports.
3. Update API and UI wording.

## Tests
No tests requested. Existing tests are not run for this task.

## Performance validation
Not applicable; this is an export-path change outside request routing.

## Security considerations
The output remains an encrypted key bundle. Installation directories may be unwritable; errors must identify the destination and preserve explicit-path override.

## Migration considerations
None. Existing backups and exports are unchanged.

## Known risks
Standard installations may restrict writes beside the executable. Users can select a different full path; if needed, the data-directory fallback applies only when executable-directory discovery fails.

## Definition of done
An empty key-export path targets the directory containing `loadoutd.exe`; an explicit path still wins; non-key backups remain unchanged; user-facing help describes the new default.

## Progress
- [x] Read the required project documents and inspect the export implementation.
- [x] Implement and update documentation.
- [ ] Move this plan to completed.

## Decisions
- Use the daemon executable location, because the daemon performs the file write and is the runtime authority. Keep data-directory fallback for an unavailable executable path.
