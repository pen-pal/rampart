# Getting Support

Rampart is a community project maintained on volunteer time. Here's the
fastest path depending on what you need.

## "How do I…?" / setup & usage questions

Use **[GitHub Discussions](https://github.com/pen-pal/rampart/discussions)**,
not the issue tracker. Before posting:

- Read [`docs/SETUP.md`](docs/SETUP.md) — covers Docker, bare binary, and the
  dev loop, plus a "common issues" section.
- Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) if your question is about
  how something works internally.
- Check existing Discussions and closed issues.

When asking, include: how you're running Rampart (image tag / binary), your
Postgres version, the monitor config involved, and any logs with
`RUST_LOG=rampart=debug`.

## Bug reports

Open an issue with the **Bug report** template. A reproducible bug with steps
gets fixed; a vague "it doesn't work" usually gets sent back for details.

## Feature requests

Open an issue with the **Feature request** template — but first read the
**Scope** section of [`CONTRIBUTING.md`](CONTRIBUTING.md). Rampart is
deliberately bounded; enterprise-observability asks are closed as `wontfix`.

## Security issues

**Never** in public. Follow [`SECURITY.md`](SECURITY.md).

## Commercial / priority support

There is no paid support tier. If the project has a funding page (see the
Sponsor button), sponsoring helps sustain maintenance but does not buy an SLA.

## Response expectations

This is best-effort. Maintainers triage in batches. A clear, reproducible
report with the right details is the single biggest thing you can do to get a
fast answer.
