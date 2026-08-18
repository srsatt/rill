import type { LoginPageModel } from "../../generated/render-contract";
import { card, cardContent, cardDescription, cardHeader } from "../server/solid-ui";

export function Login(props: { page: LoginPageModel }) {
  return (
    <main id="main-content" class="auth-shell">
      {card(<>
        {cardHeader(<>
          <a class="wordmark" href="/" aria-label="Rill home">Rill</a>
          <h1 class="text-2xl font-semibold tracking-tight">{props.page.title}</h1>
          {cardDescription("Sign in to your personal news stream.")}
        </>)}
        {cardContent(<>
          {props.page.error && <div role="alert" class="form-alert"><strong>Sign-in failed.</strong> {props.page.error}</div>}
          <form method="post" action="/login" class="form-stack">
            <label class="field-label" for="login">Username or email</label>
            <input class="field-input" id="login" name="login" autocomplete="username" required />
            <label class="field-label" for="password">Password</label>
            <input class="field-input" id="password" name="password" type="password" autocomplete="current-password" required />
            <button type="submit" class="primary-action">Sign in</button>
          </form>
        </>)}
      </>, "auth-card")}
    </main>
  );
}
