use axum::response::Html;

pub fn render_index_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>nanobot-rs control room</title>
    <style>
      :root {
        --paper: #f6efe2;
        --ink: #21313d;
        --muted: #6d6a63;
        --accent: #c9622f;
        --panel: rgba(255, 251, 245, 0.82);
        --line: rgba(33, 49, 61, 0.14);
        --shadow: 0 22px 60px rgba(75, 50, 28, 0.15);
      }

      * {
        box-sizing: border-box;
      }

      body {
        margin: 0;
        min-height: 100vh;
        color: var(--ink);
        background:
          radial-gradient(circle at top left, rgba(201, 98, 47, 0.16), transparent 28rem),
          linear-gradient(180deg, #f8f2e7 0%, #efe4d1 100%);
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
      }

      body::before {
        content: "";
        position: fixed;
        inset: 0;
        pointer-events: none;
        background-image:
          linear-gradient(rgba(33, 49, 61, 0.03) 1px, transparent 1px),
          linear-gradient(90deg, rgba(33, 49, 61, 0.03) 1px, transparent 1px);
        background-size: 2rem 2rem;
        mask-image: linear-gradient(180deg, rgba(0, 0, 0, 0.32), transparent 85%);
      }

      #app {
        width: min(92vw, 72rem);
        margin: 0 auto;
        padding: 3rem 0 4rem;
      }

      .hero {
        display: grid;
        gap: 0.75rem;
        margin-bottom: 1.75rem;
      }

      .eyebrow {
        color: var(--accent);
        text-transform: uppercase;
        letter-spacing: 0.18em;
        font-size: 0.78rem;
      }

      h1 {
        margin: 0;
        font-size: clamp(2.5rem, 5vw, 4.8rem);
        line-height: 0.92;
        font-weight: 700;
      }

      .deck {
        width: min(44rem, 100%);
        margin: 0;
        color: var(--muted);
        font-size: 1.05rem;
        line-height: 1.6;
      }

      .shell {
        display: grid;
        gap: 1rem;
        padding: 1rem;
        border: 1px solid var(--line);
        border-radius: 1.5rem;
        background: var(--panel);
        backdrop-filter: blur(12px);
        box-shadow: var(--shadow);
      }

      #transcript {
        min-height: 22rem;
        padding: 1rem;
        border: 1px dashed var(--line);
        border-radius: 1rem;
        background: rgba(255, 255, 255, 0.48);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        white-space: pre-wrap;
      }

      #composer {
        display: grid;
        gap: 0.8rem;
      }

      #message-input {
        min-height: 8rem;
        resize: vertical;
        border: 1px solid var(--line);
        border-radius: 1rem;
        padding: 1rem;
        font: inherit;
        color: var(--ink);
        background: rgba(255, 255, 255, 0.72);
      }

      #send-button {
        justify-self: start;
        border: 0;
        border-radius: 999px;
        padding: 0.85rem 1.35rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
        color: #fff8ef;
        background: linear-gradient(135deg, #c9622f, #a4461f);
        cursor: pointer;
      }

      @media (max-width: 720px) {
        #app {
          width: min(94vw, 40rem);
          padding-top: 2rem;
        }

        .shell {
          padding: 0.85rem;
          border-radius: 1.1rem;
        }
      }
    </style>
  </head>
  <body>
    <main id="app">
      <header class="hero">
        <div class="eyebrow">Local Operator Console</div>
        <h1>nanobot-rs control room</h1>
        <p class="deck">A minimal browser surface for the Rust agent. Text in, text out, same workspace brain underneath.</p>
      </header>
      <section class="shell">
        <section id="transcript"></section>
        <form id="composer">
          <textarea id="message-input" placeholder="Ask nanobot-rs to inspect, edit, or research."></textarea>
          <button id="send-button" type="submit">Send</button>
        </form>
      </section>
    </main>
  </body>
</html>"#
        .to_string()
}

pub async fn index() -> Html<String> {
    Html(render_index_html())
}
