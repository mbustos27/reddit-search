// Everything from the cache is user-written text. It is only ever placed with textContent, never parsed as HTML.
"use strict";

const $ = (id) => document.getElementById(id);

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

async function api(path) {
  // The marker header tells the server this request comes from its own page.
  const res = await fetch(path, { headers: { Accept: "application/json", "X-Rdt": "1" } });
  return res.json();
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

function row(href, title, meta, snippet, current) {
  const a = el("a", "row");
  a.href = href;
  if (current) a.setAttribute("aria-current", "true");
  a.append(el("span", "title", title), el("span", "meta", meta));
  if (snippet) a.append(el("span", "snippet", snippet));
  return a;
}

async function showList(currentId) {
  const data = await api("/api/threads");
  const list = $("list");
  clear(list);
  list.append(el("div", "heading", `Cached threads (${data.threads.length})`));
  if (!data.threads.length) list.append(el("div", "row muted", "Nothing cached yet. Read a thread with: rdt thread <url>"));
  for (const t of data.threads) {
    const meta = `r/${t.subreddit} · ${t.created} · ${t.comments} comments · fetched ${t.fetched}`;
    list.append(row(`#t=${t.id}`, t.title, meta, null, t.id === currentId));
  }
}

async function showSearch(query) {
  const list = $("list");
  clear(list);
  list.append(el("div", "heading", `Search: ${query}`));
  const data = await api(`/api/search?q=${encodeURIComponent(query)}`);
  if (!data.ok) {
    list.append(el("div", "row muted", "No cached thread matches."));
  } else {
    for (const r of data.results) {
      const meta = `r/${r.subreddit} · ${r.created} · score ${r.score ?? "?"} · matched ${r.matched.join(", ") || "-"}${r.cached ? "" : " · post only, comments not fetched"}`;
      list.append(row(`#t=${r.id.replace(/^t3_/, "")}`, r.title, meta, r.snippet, false));
    }
  }
  const back = el("a", "row muted", "Back to all cached threads");
  back.href = "#";
  list.append(back);
}

function facts(t) {
  const p = el("p", "facts");
  const parts = [
    `r/${t.subreddit}`,
    t.author ? `u/${t.author}` : "[unknown author]",
    `${t.created}`,
    `score ${t.score ?? "?"}`,
    `${t.num_comments ?? "?"} comments on Reddit, ${t.comments_loaded} cached`,
    `fetched ${t.fetched}`,
  ];
  if (t.scores_as_of) parts.push(`scores as of ${t.scores_as_of}`);
  if (t.flair) parts.push(`flair: ${t.flair}`);
  if (t.state !== "live") parts.push(`state: ${t.state}`);
  p.append(el("span", null, parts.join(" · ") + " · "));
  const link = el("a", null, "open on Reddit");
  link.href = t.url; // built by the tool from the thread id, never from thread text
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  p.append(link);
  return p;
}

async function showThread(id) {
  const view = $("view");
  clear(view);
  const t = await api(`/api/thread?id=${encodeURIComponent(id)}`);
  if (!t.ok) {
    // Only a plain id is ever turned into a command to copy; anything else in the address is ignored.
    if (/^[a-z0-9]{1,16}$/.test(id)) {
      view.append(el("p", "empty", "This thread's comments are not in the cache. This window does not fetch; to read it, run:"));
      view.append(el("pre", "body id", `rdt thread t3_${id}`));
    } else {
      view.append(el("p", "empty", "That is not a thread id."));
    }
    return;
  }
  view.append(el("h1", null, t.title), facts(t));
  if (t.note) view.append(el("p", "note", t.note));
  const post = el("section", "post");
  post.append(el("pre", "body", t.state !== "live" ? `[${t.state}: text not available]` : t.selftext || (t.link ? `[link post: ${t.link}]` : "[no text]")));
  view.append(post);
  for (const c of t.comments) {
    const box = el("section", "comment");
    box.style.setProperty("--depth", Math.min(c.depth, 12));
    const who = el("div", "who");
    who.append(el("b", null, c.author ? `u/${c.author}` : `[${c.state}]`));
    who.append(el("span", null, ` · ${c.score ?? 0} points · ${c.created}${c.edited ? ` (edited ${c.edited})` : ""} · `));
    who.append(el("span", "id", c.id));
    if (c.is_op) who.append(el("span", "tag op", "OP"));
    if (c.state !== "live") who.append(el("span", "tag gone", c.state));
    box.append(who, el("pre", "body", c.state === "live" ? c.body : ""));
    view.append(box);
  }
  const o = t.omitted;
  const left = [];
  if (o.removed_or_deleted) left.push(`${o.removed_or_deleted} removed or deleted`);
  if (o.not_loaded) left.push(`${o.not_loaded} not loaded from the archive`);
  if (left.length) view.append(el("p", "muted", `Not shown: ${left.join(", ")}.`));
  view.scrollTop = 0;
}

async function route() {
  let hash = location.hash.slice(1);
  try {
    hash = decodeURIComponent(hash);
  } catch {
    // A hand-typed address with a stray percent sign: use it as written.
  }
  if (hash.startsWith("q=")) {
    $("q").value = hash.slice(2);
    await showSearch(hash.slice(2));
  } else if (hash.startsWith("t=")) {
    const id = hash.slice(2);
    if (!$("list").querySelector(".row")) await showList(id);
    for (const a of $("list").querySelectorAll(".row")) {
      if (a.getAttribute("href") === `#t=${id}`) a.setAttribute("aria-current", "true");
      else a.removeAttribute("aria-current");
    }
    await showThread(id);
  } else {
    $("q").value = "";
    await showList(null);
    clear($("view"));
    $("view").append(el("p", "empty", "Pick a thread on the left, or search the cache. This window only reads what rdt has already fetched."));
  }
}

$("search").addEventListener("submit", (e) => {
  e.preventDefault();
  const q = $("q").value.trim();
  location.hash = q ? `q=${encodeURIComponent(q)}` : "";
});

$("check").addEventListener("click", async () => {
  $("archive").textContent = "asking the archive (one request)…";
  try {
    const data = await api("/api/sources");
    $("archive").textContent = data.ok ? data.lines.join("  |  ") : data.error || "the check failed";
  } catch {
    $("archive").textContent = "the check failed";
  }
});

window.addEventListener("hashchange", route);

(async () => {
  const s = await api("/api/stats");
  $("stats").textContent = `${s.threads} threads, ${s.comments} comments cached`;
  await route();
})();
