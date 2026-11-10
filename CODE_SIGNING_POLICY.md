# Code signing policy / Politique de signature du code

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

Signature du code gratuite fournie par [SignPath.io](https://about.signpath.io), certificat de la [SignPath Foundation](https://signpath.org).

## What is signed / Ce qui est signé

Only the Windows installer (`FPS Cagoule_x.y.z_x64-setup.exe`) and the application executable built by the
[GitHub Actions workflow](.github/workflows) of this repository from the tagged source code.
Third-party binaries shipped with the app keep their own publisher signature (PresentMon, signed by Intel).

Seuls l'installateur Windows et l'exécutable de l'app, compilés par GitHub Actions à partir du code source de ce dépôt, sont signés.

## Team roles / Rôles

| Role | Member |
|---|---|
| Committers and reviewers | [Lucas Chavanne](https://github.com/lucas-chvnn-info) |
| Approvers | [Lucas Chavanne](https://github.com/lucas-chvnn-info) |

All team members use multi-factor authentication on GitHub and SignPath.

## Privacy policy / Confidentialité

FPS Cagoule does not collect, store or send any personal data or telemetry. Everything it measures (analysis, benchmark,
FPS statistics, settings) stays in `%APPDATA%\FPS Cagoule` on the user's PC.

The app only connects to the network for these user-visible features:

- **Updates**: checks `github.com/lucas-chvnn-info/fps-cagoule/releases` for a new version (can be turned off in Settings).
- **NVIDIA driver**: asks NVIDIA's public driver lookup service for the latest driver of the installed card, when the Drivers page is opened.
- **Ping test**: sends ICMP echo requests to the user's router and to `1.1.1.1` when the user starts the test.
- **DNS test**: sends DNS queries for public domains to the current DNS server and to Cloudflare, Google and Quad9 when the user starts the test.
- **Windows Update drivers**: uses the Windows Update API of the system when the user searches for driver updates.

FPS Cagoule ne collecte ni n'envoie aucune donnée personnelle. Les seules connexions sont celles listées ci-dessus, toutes visibles et déclenchées par l'utilisateur (sauf la recherche de mise à jour, désactivable).

## System changes / Modifications du système

Every Windows setting changed by the app is shown to the user before being applied, can be undone from the app
(original values are saved, "Undo all" and "Restore everything" buttons), and a Windows restore point is created first by default.
Settings that lower Windows security (Spectre/Meltdown mitigations, memory integrity, Defender exclusions) are grouped in a
separate "risky zone": never applied in batch, each one requires its own confirmation with a warning, and each one can be reverted.
The app is uninstalled from Windows Settings → Apps like any other program.
