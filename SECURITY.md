# Security

Arbiter runs coding agents on your computer, so security reports matter a lot to us.

## Reporting a problem

Please do **not** open a public issue for a security problem. Use GitHub's private vulnerability reporting instead: **Security → Report a vulnerability** on this repository. Include what you found, how to reproduce it, and what an attacker could do with it.

We aim to acknowledge reports within a few days and to fix confirmed issues in the next release.

## Scope

In scope:
- the local service (`arbiterd`) and its API;
- the desktop app;
- the updater;
- anything that could let a web page, a repository, a document or an agent's output take actions you did not approve;
- anything that could leak code, secrets or memory.

## Design commitments

- The service listens only on `127.0.0.1`, and every route except health needs a per-install token.
- Updates are verified against a signing key built into the app before they install.
- Agents' web access, pushes, pull requests and paid usage each need explicit consent.
