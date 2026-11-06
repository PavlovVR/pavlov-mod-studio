# Security

Pavlov Mod Studio handles people's mod.io tokens, uploads to their mods and installs its own updates, so security problems matter here.

## Report a problem

Please don't describe a security problem in a public issue.

Use **Report a vulnerability** on the [Security tab](https://github.com/PavlovVR/pavlov-mod-studio/security). If that isn't available, [open an issue](https://github.com/PavlovVR/pavlov-mod-studio/issues/new/choose) saying only that you have a security problem to report, and a maintainer will arrange a private way to share it.

Include what you found, how to reproduce it and what someone could do with it. Please give us a reasonable time to fix it before making it public.

## What counts

- A way to read or leak someone's mod.io token
- A way to get an unsigned or tampered update installed
- A way to get another mod's files past the content check
- A way for a mod, a source plugin or a mod.io response to run code on someone's PC

Problems with mod.io or with Pavlov itself belong with them.

## Supported versions

Only the [latest release](https://github.com/PavlovVR/pavlov-mod-studio/releases/latest) gets fixes. The app updates itself by default.

## How the app protects you

- Your token is encrypted for your Windows account and is only ever sent to mod.io.
- Every update is signed, and the app refuses anything that doesn't match.
- Every build is checked before upload, and a build containing another mod's files stops.

If you've pasted a token somewhere public, delete it on your [mod.io access page](https://mod.io/me/access) and make a new one.
