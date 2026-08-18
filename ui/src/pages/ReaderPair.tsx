import type { ReaderPairPageModel } from "../../generated/render-contract";

export function ReaderPair(props: { page: ReaderPairPageModel; csrfToken: string }) {
  return (
    <main class="reader auth-shell">
      <h1>{props.page.title}</h1>
      <p>Enter the one-time code shown in Rill on your main device.</p>
      {props.page.error && <p role="alert">{props.page.error}</p>}
      <form method="post" action="/reader/pair" autocomplete="off">
        <input type="hidden" name="csrf_token" value={props.csrfToken} />
        <label>Pairing code <input name="code" inputmode="text" maxlength="9" required /></label>
        <button type="submit">Pair reader</button>
      </form>
    </main>
  );
}
