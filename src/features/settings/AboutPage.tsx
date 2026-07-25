import { useState } from "react";
import { Button, Card } from "react-bootstrap";
// lucide v1 dropped brand icons, so no GitHub mark — GitBranch stands in.
import {
  Bug,
  ChevronLeft,
  ExternalLink,
  GitBranch,
  User,
  type LucideIcon,
} from "lucide-react";
import { BrandMark } from "../../components/BrandMark";
import { openExternal } from "../../lib/openExternal";
import type { Navigate } from "../../navigation";

const AUTHOR = "PheeLeep";
const AUTHOR_URL = "https://github.com/PheeLeep";
const REPO_URL = "https://github.com/PheeLeep/ParolaSSH";
const ISSUES_URL = `${REPO_URL}/issues`;
/** Redirects to avatars.githubusercontent.com; both hosts are in the CSP. */
const AUTHOR_AVATAR = `${AUTHOR_URL}.png?size=160`;

const BUILT_WITH = [
  "Tauri 2",
  "Rust",
  "React 19",
  "TypeScript",
  "Bootstrap 5",
  "TanStack Table",
  "xterm.js",
  "Lucide",
];

export function AboutPage({ onNavigate }: { onNavigate: Navigate }) {
  return (
    <div className="page page--narrow">
      <Button
        variant="link"
        size="sm"
        className="p-0 mb-3 text-decoration-none text-body-secondary"
        onClick={() => onNavigate({ kind: "settings" })}
      >
        <ChevronLeft className="icon-sm" aria-hidden="true" />
        Settings
      </Button>

      <header className="about-hero">
        <span className="about-hero__mark" aria-hidden="true">
          <BrandMark />
        </span>
        <h1 className="about-hero__title">ParolaSSH</h1>
        <p className="about-hero__version">Version {__APP_VERSION__}</p>
        <p className="text-body-secondary mb-0">
          A desktop SSH console — saved hosts, key management, and terminals in
          one window.
        </p>
      </header>

      <Card className="mb-3">
        <Card.Body className="p-2">
          <LinkRow
            Icon={User}
            avatarUrl={AUTHOR_AVATAR}
            title={AUTHOR}
            hint="Author"
            onClick={() => openExternal(AUTHOR_URL)}
          />
          <LinkRow
            Icon={GitBranch}
            title="PheeLeep/ParolaSSH"
            hint="Source repository"
            onClick={() => openExternal(REPO_URL)}
          />
          <LinkRow
            Icon={Bug}
            title="Report an issue"
            hint="Bugs and feature requests"
            onClick={() => openExternal(ISSUES_URL)}
          />
        </Card.Body>
      </Card>

      <Card>
        <Card.Body>
          <h2 className="section-title mb-3">Built with</h2>
          <div className="d-flex flex-wrap gap-1">
            {BUILT_WITH.map((item) => (
              <span key={item} className="tag-chip">
                {item}
              </span>
            ))}
          </div>
        </Card.Body>
      </Card>
    </div>
  );
}

function LinkRow({
  Icon,
  avatarUrl,
  title,
  hint,
  onClick,
}: {
  Icon: LucideIcon;
  avatarUrl?: string;
  title: string;
  hint: string;
  onClick: () => void;
}) {
  // The avatar is the one thing here that needs the network. Offline or if
  // GitHub is unreachable, fall back to the icon rather than a broken image.
  const [avatarBroken, setAvatarBroken] = useState(false);

  return (
    <button type="button" className="link-row" onClick={onClick}>
      <span className="link-row__icon" aria-hidden="true">
        {avatarUrl && !avatarBroken ? (
          <img
            className="link-row__avatar"
            src={avatarUrl}
            alt=""
            loading="lazy"
            onError={() => setAvatarBroken(true)}
          />
        ) : (
          <Icon />
        )}
      </span>
      <span className="link-row__text">
        <span className="link-row__title">{title}</span>
        <span className="link-row__hint">{hint}</span>
      </span>
      <ExternalLink className="icon-sm link-row__arrow" aria-hidden="true" />
    </button>
  );
}
