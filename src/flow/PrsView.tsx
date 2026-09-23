import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { formatRelative } from "./format";
import { OPEN_PR_WINDOWS, useOpenPrs, type OpenPrWindow } from "./useOpenPrs";

export function PrsView() {
  const [days, setDays] = useState<OpenPrWindow>(30);
  const { list, loading, error } = useOpenPrs(days);
  const truncated = list != null && list.total > list.prs.length;

  return (
    <div>
      <div className="flow-pr-windows" role="group" aria-label="Updated within">
        {OPEN_PR_WINDOWS.map((window) => (
          <button
            key={window}
            type="button"
            data-active={days === window}
            onClick={() => setDays(window)}
          >
            {window} days
          </button>
        ))}
      </div>
      {loading && !list && <p className="flow-empty">Loading…</p>}
      {error && <p className="flow-error">{error}</p>}
      {!loading && !error && (!list || list.prs.length === 0) && (
        <p className="flow-empty">No open pull requests in the last {days} days</p>
      )}
      {truncated && list && (
        <p className="flow-pr-count">
          {list.prs.length} of {list.total}
        </p>
      )}
      {list && list.prs.length > 0 && (
        <ul className="flow-pr-list">
          {list.prs.map((pr) => (
            <li key={`${pr.repo}#${pr.number}`}>
              <button
                type="button"
                className="flow-pr-row"
                onClick={() => {
                  openUrl(pr.url).catch(() => undefined);
                }}
              >
                <div className="flow-pr-title">
                  <span>{pr.title}</span>
                  {pr.is_draft && <span className="flow-pr-draft">Draft</span>}
                </div>
                <div className="flow-pr-meta">
                  <span>{pr.repo}</span>
                  <span>#{pr.number}</span>
                  <span>{formatRelative(pr.updated_at_ms)}</span>
                </div>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
