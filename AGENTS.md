# Project maintenance

- Update PORTING_LEDGER.md whenever a Fibril feature is adapted, implemented, deferred or deliberately omitted, or a new feature is added.
- Keep implemented behavior distinct from planned behavior. Record verification and limitations.
- Preserve Fibril's theme collection and design principles. The favicon uses the natsui pixel kitten and the dashboard uses a text wordmark; do not restore the Fibril mascot. Do not infer poor health from traffic volume.
- Label reported, derived and unavailable evidence. Never replace a failed collection with healthy zero values.
- Consumer information queries must never pull or acknowledge application messages.
- Standard mode must work with unmodified NATS. Deployment control is an optional, separate capability.
- Dashboard identities and NATS identities are separate security domains.
- Comments and documentation use neutral prose and ASCII characters.
