use axum::{Router, response::Html, routing::get};

/// Serve the static HTML frontend.
pub fn static_router() -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Minimal single-page frontend with dark/light theme support.
const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Hermes</title>
    <style>
        :root {
            --bg: #ffffff;
            --fg: #1a1a1a;
            --bg-secondary: #f5f5f5;
            --border: #e0e0e0;
            --accent: #2563eb;
            --text-secondary: #666;
        }
        [data-theme="dark"] {
            --bg: #1a1a1a;
            --fg: #e0e0e0;
            --bg-secondary: #2a2a2a;
            --border: #404040;
            --accent: #3b82f6;
            --text-secondary: #999;
        }
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg);
            color: var(--fg);
            line-height: 1.6;
        }
        .container { max-width: 900px; margin: 0 auto; padding: 1rem; }
        header {
            display: flex; justify-content: space-between; align-items: center;
            padding: 1rem 0; border-bottom: 1px solid var(--border);
        }
        header h1 { font-size: 1.4rem; }
        .theme-toggle {
            background: none; border: 1px solid var(--border); color: var(--fg);
            padding: 0.4rem 0.8rem; border-radius: 6px; cursor: pointer;
        }
        .theme-toggle:hover { background: var(--bg-secondary); }
        #messages {
            min-height: 300px; max-height: 60vh; overflow-y: auto;
            padding: 1rem 0;
        }
        .message { padding: 0.8rem; margin-bottom: 0.5rem; border-radius: 8px; }
        .message.user { background: var(--bg-secondary); margin-left: 2rem; }
        .message.assistant { background: var(--bg); margin-right: 2rem; border: 1px solid var(--border); }
        .message .role { font-size: 0.75rem; color: var(--text-secondary); margin-bottom: 0.25rem; }
        #input-area {
            display: flex; gap: 0.5rem; padding: 1rem 0;
            border-top: 1px solid var(--border);
        }
        #input {
            flex: 1; padding: 0.8rem; border: 1px solid var(--border);
            border-radius: 8px; background: var(--bg); color: var(--fg);
            font-size: 1rem; resize: none;
        }
        #send {
            padding: 0.8rem 1.5rem; background: var(--accent); color: white;
            border: none; border-radius: 8px; cursor: pointer; font-size: 1rem;
        }
        #send:hover { opacity: 0.9; }
        #send:disabled { opacity: 0.5; cursor: not-allowed; }
        .nav { display: flex; gap: 0.5rem; margin-top: 0.5rem; }
        .nav button {
            padding: 0.3rem 0.6rem; border: 1px solid var(--border);
            background: var(--bg); color: var(--fg); border-radius: 4px;
            cursor: pointer; font-size: 0.85rem;
        }
        .nav button:hover { background: var(--bg-secondary); }
        .status { font-size: 0.8rem; color: var(--text-secondary); margin-top: 0.5rem; }
        .hidden { display: none; }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>Hermes</h1>
            <button class="theme-toggle" onclick="toggleTheme()">Toggle Theme</button>
        </header>
        <div class="nav">
            <button onclick="showChat()">Chat</button>
            <button onclick="loadSessions()">Sessions</button>
            <button onclick="loadModels()">Models</button>
        </div>
        <div class="status" id="status"></div>
        <div id="sessions-view" class="hidden">
            <ul id="session-list"></ul>
        </div>
        <div id="models-view" class="hidden">
            <div id="current-model"></div>
            <ul id="provider-list"></ul>
        </div>
        <div id="chat-view">
            <div id="messages"></div>
            <div id="input-area">
                <textarea id="input" rows="2" placeholder="Type a message..." onkeydown="handleKey(event)"></textarea>
                <button id="send" onclick="sendMessage()">Send</button>
            </div>
        </div>
    </div>
    <script>
        var state = { sessionId: null };
        function setStatus(msg) { document.getElementById('status').textContent = msg; }

        function showChat() {
            document.getElementById('chat-view').classList.remove('hidden');
            document.getElementById('sessions-view').classList.add('hidden');
            document.getElementById('models-view').classList.add('hidden');
        }

        async function loadSessions() {
            document.getElementById('chat-view').classList.add('hidden');
            document.getElementById('sessions-view').classList.remove('hidden');
            document.getElementById('models-view').classList.add('hidden');
            try {
                var res = await fetch('/api/sessions');
                var data = await res.json();
                var list = document.getElementById('session-list');
                list.innerHTML = '';
                data.sessions.forEach(function(s) {
                    var li = document.createElement('li');
                    li.innerHTML = '<a href="#" onclick="openSession(\'' + s.id + '\')">' + (s.title || s.id.slice(0,8)) + '</a> (' + s.message_count + ' msgs)';
                    list.appendChild(li);
                });
                if (data.sessions.length === 0) list.innerHTML = '<li>No sessions yet.</li>';
            } catch (e) { setStatus('Failed to load sessions: ' + e.message); }
        }

        async function loadModels() {
            document.getElementById('chat-view').classList.add('hidden');
            document.getElementById('sessions-view').classList.add('hidden');
            document.getElementById('models-view').classList.remove('hidden');
            try {
                var res = await fetch('/api/models');
                var data = await res.json();
                document.getElementById('current-model').innerHTML = '<strong>Current:</strong> ' + data.current_model;
                var ul = document.getElementById('provider-list');
                ul.innerHTML = '';
                data.providers.forEach(function(p) {
                    var li = document.createElement('li');
                    li.textContent = p.display_name + ' (' + p.default_model + ')';
                    ul.appendChild(li);
                });
            } catch (e) { setStatus('Failed to load models: ' + e.message); }
        }

        async function openSession(id) {
            state.sessionId = id;
            showChat();
            setStatus('Opened session ' + id.slice(0, 8));
            try {
                var res = await fetch('/api/sessions/' + id);
                var data = await res.json();
                var msgs = document.getElementById('messages');
                msgs.innerHTML = '';
                data.messages.forEach(function(m) { addMessage(m.role, m.content || ''); });
            } catch (e) { setStatus('Failed to load session: ' + e.message); }
        }

        function addMessage(role, content) {
            var msgs = document.getElementById('messages');
            var div = document.createElement('div');
            div.className = 'message ' + role;
            div.innerHTML = '<div class="role">' + role + '</div><div>' + escapeHtml(content) + '</div>';
            msgs.appendChild(div);
            msgs.scrollTop = msgs.scrollHeight;
        }

        function escapeHtml(text) {
            var d = document.createElement('div');
            d.textContent = text;
            return d.innerHTML;
        }

        async function sendMessage() {
            var input = document.getElementById('input');
            var sendBtn = document.getElementById('send');
            var msg = input.value.trim();
            if (!msg) return;

            input.value = '';
            sendBtn.disabled = true;
            addMessage('user', msg);

            try {
                var body = JSON.stringify({ message: msg, session_id: state.sessionId });
                var res = await fetch('/api/chat', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: body,
                });
                var data = await res.json();
                state.sessionId = data.session_id;
                addMessage('assistant', data.response);
                setStatus('');
            } catch (e) {
                addMessage('assistant', 'Error: ' + e.message);
            }
            sendBtn.disabled = false;
            input.focus();
        }

        function handleKey(e) {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
        }

        function toggleTheme() {
            var html = document.documentElement;
            var current = html.getAttribute('data-theme');
            html.setAttribute('data-theme', current === 'dark' ? 'light' : 'dark');
        }

        if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) {
            document.documentElement.setAttribute('data-theme', 'dark');
        }
    </script>
</body>
</html>"##;
