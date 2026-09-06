# Working in this repo

**Never commit or push unless asked.** Leave finished work in the working tree and say what is there. The same goes for
anything else that leaves this machine:
publishing a release, triggering a workflow, opening a pull request.

**Verify on the real app, not just `cargo test`.** Almost every bug here has been a platform behaving differently from
its documentation, which a unit test cannot see. `scripts/` holds the harnesses: three that drive the built binary
through the paths needing a real window, a real notification daemon or a real restart, and `chatjs-test.js`, which runs
the injected script in Node against a stand-in page.

**When only a human can check it, stop and say so.** Signing in, clicking a notification popup, reading a real incoming
message, launching a macOS or Windows bundle: none of these can be driven from here. Do the work that does not depend on
them, then say plainly what is left and what to do — do not guess at the result, and do not report something as working
because the parts around it are.

**Do not build the AppImage locally** — it takes over fifteen minutes. Run the
`release` workflow by hand and take the artifact.

`docs/Development.md` is the reference: architecture, the platform quirks that cost real time to find, CI, and where
things stand.
