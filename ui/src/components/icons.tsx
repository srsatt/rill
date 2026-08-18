type IconName = "bookmark" | "bookmark-check" | "external-link" | "eye" | "eye-off" | "message-circle" | "search";

function Icon(props: { name: IconName }) {
  return <svg class="rill-icon" width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <use href={`/static/lucide-rill.svg#${props.name}`} />
  </svg>;
}

export function BookmarkIcon() {
  return <Icon name="bookmark" />;
}

export function BookmarkCheckIcon() {
  return <Icon name="bookmark-check" />;
}

export function ExternalLinkIcon() {
  return <Icon name="external-link" />;
}

export function MessageCircleIcon() {
  return <Icon name="message-circle" />;
}

export function SearchIcon() {
  return <Icon name="search" />;
}

export function EyeIcon() {
  return <Icon name="eye" />;
}

export function EyeOffIcon() {
  return <Icon name="eye-off" />;
}
