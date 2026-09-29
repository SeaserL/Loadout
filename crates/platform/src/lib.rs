// SPDX-License-Identifier: Apache-2.0

//! Platform services behind a trait (ADR-001). v1 ships only the Windows implementation:
//! data directories, per-user single-instance mutex, HKCU Run autostart and user SID lookup.
