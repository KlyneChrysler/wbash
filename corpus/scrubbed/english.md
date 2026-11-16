# Release notes

This release focuses on predictability. The dispatcher now applies the policy
gate before any filesystem or process work, so a denied call cannot leave partial
side effects. Paths are canonicalized once and reused, which removes a class of
symlink races.

Output rendering is deterministic. Records are ranked by value per token, while
error, exit, and timeout records are always preserved. When the budget cannot fit
everything, the response includes an explicit elision with a handle that resolves
to the exact omitted bytes.

Operators asked for clearer failures. Each error now names the violated rule and
suggests a concrete next step, for example a smaller line range or a different
edit anchor. Logging remains off by default and never records secrets.
