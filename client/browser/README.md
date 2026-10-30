# Diagnostic browser client

A same-origin page for the ADR-104 test client. The identity server serves it
at `/` when `CORDEL_BROWSER_ORIGIN` is set to that exact origin. The session
secret stays in the `HttpOnly` cookie. The page keeps the CSRF value in memory
only.

This slice covers invite redemption, login, logout, text channels, and live
text. It does not cover voice, screen sharing, admin controls, or images.
