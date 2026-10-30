// Diagnostic browser client. The session secret stays in the HttpOnly cookie.
// This script keeps only the CSRF value in memory.

let csrf = "";
let account = null;
let channelId = "";
let socket = null;
let seen = new Set();

const statusLine = document.querySelector("#status");
const enter = document.querySelector("#enter");
const chat = document.querySelector("#chat");
const who = document.querySelector("#who");
const logoutButton = document.querySelector("#logout");
const channelSelect = document.querySelector("#channels");
const messageList = document.querySelector("#messages");

function report(error) {
  const code = error && error.message ? error.message : "request_failed";
  statusLine.textContent = code;
}

function clearStatus() {
  statusLine.textContent = "";
}

async function api(path, method, body) {
  const headers = {};
  if (csrf) headers["X-CSRF-Token"] = csrf;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    const code = payload && payload.error ? payload.error.code : String(response.status);
    throw new Error(code);
  }
  return payload;
}

function showChat(on) {
  enter.hidden = on;
  chat.hidden = !on;
  logoutButton.hidden = !on;
  who.textContent = on && account ? account.login : "";
}

async function boot() {
  const issued = await api("/v1/browser/csrf", "GET");
  csrf = issued.csrf_token;
  try {
    account = await api("/v1/me", "GET");
    await openChat();
  } catch (error) {
    if (error.message !== "unauthenticated") report(error);
    showChat(false);
  }
}

async function openChat() {
  clearStatus();
  showChat(true);
  await loadChannels();
  connect();
}

async function loadChannels() {
  const list = await api(`/v1/communities/${account.community_id}/channels`, "GET");
  const texts = list.channels.filter((channel) => channel.kind === "text");
  channelSelect.replaceChildren();
  for (const channel of texts) {
    const option = document.createElement("option");
    option.value = channel.channel_id;
    option.textContent = channel.name;
    channelSelect.append(option);
  }
  if (texts.length === 0) {
    channelId = "";
    messageList.replaceChildren();
    return;
  }
  channelId = channelSelect.value;
  await loadHistory();
  subscribe();
}

async function loadHistory() {
  messageList.replaceChildren();
  seen = new Set();
  if (!channelId) return;
  const page = await api(`/v1/channels/${channelId}/messages?limit=50`, "GET");
  for (const message of page.messages) paint(message);
}

function paint(message) {
  if (!message || (message.kind && message.kind !== "message")) return;
  if (message.channel_id && message.channel_id !== channelId) return;
  const key = String(message.seq);
  if (seen.has(key)) return;
  seen.add(key);
  const item = document.createElement("li");
  const seq = document.createElement("span");
  seq.className = "seq";
  seq.textContent = key;
  item.append(seq, document.createTextNode(message.body || ""));
  messageList.append(item);
}

function connect() {
  if (socket) socket.close();
  const scheme = location.protocol === "https:" ? "wss://" : "ws://";
  socket = new WebSocket(scheme + location.host + "/v1/events");
  socket.addEventListener("open", () => {
    socket.send(JSON.stringify({
      schema_version: 1,
      type: "browser.authenticate",
      csrf_token: csrf,
    }));
  });
  socket.addEventListener("message", (event) => {
    const frame = JSON.parse(event.data);
    if (frame.kind === "ready") subscribe();
    if (frame.kind === "message") paint(frame);
    if (frame.kind === "error") statusLine.textContent = frame.code || "socket_error";
  });
  socket.addEventListener("close", () => {
    if (!account) return;
    setTimeout(connect, 1000);
  });
}

function subscribe() {
  if (!socket || socket.readyState !== WebSocket.OPEN || !channelId) return;
  socket.send(JSON.stringify({ v: 1, kind: "subscribe", channel_id: channelId }));
}

document.querySelector("#login").addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(event.target);
  try {
    const session = await api("/v1/sessions", "POST", {
      login: data.get("login"),
      password: data.get("password"),
    });
    csrf = session.csrf_token;
    account = await api("/v1/me", "GET");
    await openChat();
  } catch (error) {
    report(error);
  }
});

document.querySelector("#invite").addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(event.target);
  try {
    const session = await api("/v1/accounts/from-invite", "POST", {
      invite_code: data.get("invite_code"),
      login: data.get("login"),
      password: data.get("password"),
    });
    csrf = session.csrf_token;
    account = await api("/v1/me", "GET");
    await openChat();
  } catch (error) {
    report(error);
  }
});

logoutButton.addEventListener("click", async () => {
  try {
    await api("/v1/sessions/current", "DELETE");
  } catch (error) {
    report(error);
  }
  account = null;
  if (socket) socket.close();
  socket = null;
  showChat(false);
  await boot();
});

channelSelect.addEventListener("change", async () => {
  channelId = channelSelect.value;
  try {
    await loadHistory();
    subscribe();
  } catch (error) {
    report(error);
  }
});

document.querySelector("#channel").addEventListener("submit", async (event) => {
  event.preventDefault();
  const name = new FormData(event.target).get("name");
  if (!name) return;
  try {
    await api(`/v1/communities/${account.community_id}/channels`, "POST", {
      kind: "text",
      name,
    });
    event.target.reset();
    await loadChannels();
  } catch (error) {
    report(error);
  }
});

document.querySelector("#composer").addEventListener("submit", async (event) => {
  event.preventDefault();
  const body = new FormData(event.target).get("body");
  try {
    await api(`/v1/channels/${channelId}/messages`, "POST", {
      client_message_id: crypto.randomUUID(),
      body,
    });
    event.target.reset();
  } catch (error) {
    report(error);
  }
});

boot().catch(report);
