use axum::response::{Html, IntoResponse};

pub async fn serve_ui() -> impl IntoResponse {
    Html(UI_HTML)
}

pub const UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="it" class="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Athena NGSI-LD Broker Dashboard</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;600&family=Plus+Jakarta+Sans:wght@400;500;600;700&display=swap" rel="stylesheet">
    <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" integrity="sha256-p4NxAoJBhIIN+hmNHrzRCf9tD/miZyoHS5obTRR9BMY=" crossorigin="" />
    <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js" integrity="sha256-20nQCchB9co0qIjJZRGuk2/Z9VM+kNiyxNV1lvTlZBo=" crossorigin=""></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js"></script>
    <style>
        :root {
            --bg-base: #0B0F19;
            --bg-surface: #111827;
            --bg-card: #1F2937;
            --border: #374151;
            --primary: #3B82F6;
            --primary-hover: #2563EB;
            --accent: #10B981;
            --danger: #EF4444;
            --warning: #F59E0B;
            --text-main: #F9FAFB;
            --text-muted: #9CA3AF;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        body {
            font-family: 'Plus Jakarta Sans', -apple-system, BlinkMacSystemFont, sans-serif;
            background-color: var(--bg-base);
            color: var(--text-main);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
        }

        header {
            background-color: var(--bg-surface);
            border-bottom: 1px solid var(--border);
            padding: 1rem 1.5rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
            position: sticky;
            top: 0;
            z-index: 1000;
        }

        .brand {
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }

        .logo-icon {
            width: 36px;
            height: 36px;
            background: linear-gradient(135deg, #3B82F6, #8B5CF6);
            border-radius: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
            font-weight: 700;
            font-size: 1.2rem;
            color: white;
            box-shadow: 0 0 15px rgba(59, 130, 246, 0.4);
        }

        .brand h1 {
            font-size: 1.25rem;
            font-weight: 700;
            letter-spacing: -0.02em;
            background: linear-gradient(to right, #F9FAFB, #93C5FD);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }

        .brand .badge {
            font-size: 0.7rem;
            background: rgba(59, 130, 246, 0.2);
            border: 1px solid rgba(59, 130, 246, 0.4);
            color: #93C5FD;
            padding: 0.15rem 0.5rem;
            border-radius: 9999px;
            font-family: 'JetBrains Mono', monospace;
        }

        .header-status {
            display: flex;
            align-items: center;
            gap: 1rem;
        }

        .status-pill {
            display: inline-flex;
            align-items: center;
            gap: 0.4rem;
            padding: 0.3rem 0.75rem;
            background-color: var(--bg-card);
            border: 1px solid var(--border);
            border-radius: 9999px;
            font-size: 0.8rem;
            font-weight: 500;
        }

        .status-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background-color: var(--accent);
            box-shadow: 0 0 8px var(--accent);
        }

        /* Navigation Tabs */
        .tabs {
            background-color: var(--bg-surface);
            border-bottom: 1px solid var(--border);
            display: flex;
            padding: 0 1.5rem;
            gap: 1rem;
        }

        .tab-btn {
            background: none;
            border: none;
            color: var(--text-muted);
            padding: 0.85rem 0.5rem;
            font-size: 0.9rem;
            font-weight: 600;
            cursor: pointer;
            border-bottom: 2px solid transparent;
            transition: all 0.15s ease;
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        .tab-btn:hover {
            color: var(--text-main);
        }

        .tab-btn.active {
            color: var(--primary);
            border-bottom-color: var(--primary);
        }

        /* Main Container */
        main {
            flex: 1;
            padding: 1.5rem;
            max-width: 1400px;
            margin: 0 auto;
            width: 100%;
        }

        .tab-panel {
            display: none;
        }

        .tab-panel.active {
            display: block;
        }

        /* Grid Layouts */
        .grid-split {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 1.5rem;
        }

        @media (max-width: 1024px) {
            .grid-split {
                grid-template-columns: 1fr;
            }
        }

        /* Cards */
        .card {
            background-color: var(--bg-surface);
            border: 1px solid var(--border);
            border-radius: 12px;
            padding: 1.25rem;
            margin-bottom: 1.5rem;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.2);
        }

        .card-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 1rem;
            padding-bottom: 0.75rem;
            border-bottom: 1px solid var(--border);
        }

        .card-title {
            font-size: 1rem;
            font-weight: 700;
            color: var(--text-main);
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        /* Form Controls */
        .form-group {
            margin-bottom: 1rem;
        }

        .form-label {
            display: block;
            font-size: 0.8rem;
            font-weight: 600;
            color: var(--text-muted);
            margin-bottom: 0.35rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }

        .form-input, .form-select, .form-textarea {
            width: 100%;
            background-color: var(--bg-card);
            border: 1px solid var(--border);
            border-radius: 6px;
            padding: 0.6rem 0.8rem;
            color: var(--text-main);
            font-family: inherit;
            font-size: 0.9rem;
            transition: border-color 0.15s ease;
        }

        .form-input:focus, .form-select:focus, .form-textarea:focus {
            outline: none;
            border-color: var(--primary);
            box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.2);
        }

        .form-input.code, .form-textarea.code {
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.85rem;
        }

        .btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 0.5rem;
            padding: 0.6rem 1.2rem;
            border-radius: 6px;
            font-size: 0.9rem;
            font-weight: 600;
            cursor: pointer;
            transition: all 0.15s ease;
            border: none;
        }

        .btn-primary {
            background-color: var(--primary);
            color: white;
        }

        .btn-primary:hover {
            background-color: var(--primary-hover);
            box-shadow: 0 0 12px rgba(59, 130, 246, 0.4);
        }

        .btn-secondary {
            background-color: var(--bg-card);
            border: 1px solid var(--border);
            color: var(--text-main);
        }

        .btn-secondary:hover {
            background-color: #2D3748;
        }

        .btn-success {
            background-color: var(--accent);
            color: white;
        }

        .btn-sm {
            padding: 0.35rem 0.75rem;
            font-size: 0.8rem;
        }

        /* Results & Code Viewer */
        .code-container {
            position: relative;
            background-color: #050811;
            border: 1px solid var(--border);
            border-radius: 8px;
            overflow: hidden;
        }

        .code-header {
            background-color: #0d1322;
            padding: 0.4rem 0.8rem;
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
            font-size: 0.75rem;
            color: var(--text-muted);
            font-family: 'JetBrains Mono', monospace;
        }

        pre {
            padding: 1rem;
            overflow-x: auto;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.85rem;
            line-height: 1.5;
            color: #E2E8F0;
            max-height: 500px;
        }

        /* Map Container */
        #map {
            height: 380px;
            width: 100%;
            border-radius: 8px;
            border: 1px solid var(--border);
            z-index: 10;
        }

        /* Entity Cards List */
        .entity-card {
            background-color: var(--bg-card);
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 1rem;
            margin-bottom: 0.75rem;
            transition: transform 0.15s ease, border-color 0.15s ease;
        }

        .entity-card:hover {
            border-color: var(--primary);
            transform: translateY(-2px);
        }

        .entity-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 0.5rem;
        }

        .entity-id {
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.9rem;
            font-weight: 600;
            color: #60A5FA;
        }

        .entity-type {
            font-size: 0.75rem;
            background-color: rgba(139, 92, 246, 0.2);
            color: #C4B5FD;
            padding: 0.2rem 0.5rem;
            border-radius: 4px;
            font-weight: 600;
        }

        .entity-props {
            display: flex;
            flex-wrap: wrap;
            gap: 0.5rem;
            margin-top: 0.5rem;
        }

        .prop-tag {
            font-size: 0.75rem;
            background-color: rgba(255, 255, 255, 0.05);
            padding: 0.2rem 0.4rem;
            border-radius: 4px;
            font-family: 'JetBrains Mono', monospace;
            color: var(--text-muted);
        }

        .prop-tag strong {
            color: var(--text-main);
        }

        /* Quick Templates Chips */
        .chip-group {
            display: flex;
            flex-wrap: wrap;
            gap: 0.5rem;
            margin-bottom: 1rem;
        }

        .chip {
            background-color: var(--bg-card);
            border: 1px solid var(--border);
            padding: 0.35rem 0.75rem;
            border-radius: 9999px;
            font-size: 0.8rem;
            cursor: pointer;
            color: var(--text-muted);
            transition: all 0.15s ease;
        }

        .chip:hover, .chip.active {
            color: var(--primary);
            border-color: var(--primary);
            background-color: rgba(59, 130, 246, 0.1);
        }

        /* Toast notification */
        #toast {
            position: fixed;
            bottom: 2rem;
            right: 2rem;
            background-color: var(--accent);
            color: white;
            padding: 0.75rem 1.25rem;
            border-radius: 8px;
            font-weight: 600;
            font-size: 0.9rem;
            box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.5);
            transform: translateY(150%);
            transition: transform 0.2s ease-in-out;
            z-index: 9999;
        }

        #toast.show {
            transform: translateY(0);
        }
    </style>
</head>
<body>

    <!-- Header -->
    <header>
        <div class="brand">
            <div class="logo-icon">A</div>
            <div>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    <h1>Athena Broker</h1>
                    <span class="badge">NGSI-LD v1.8.1</span>
                </div>
                <div style="font-size: 0.75rem; color: var(--text-muted);">High-Performance Context Broker & Explorer</div>
            </div>
        </div>
        <div class="header-status">
            <div class="status-pill">
                <span class="status-dot"></span>
                <span id="broker-status-text">Healthy (Rust/Axum)</span>
            </div>
            <div class="status-pill">
                <span id="entity-count-badge">0 Entità</span>
            </div>
        </div>
    </header>

    <!-- Navigation Tabs -->
    <div class="tabs">
        <button class="tab-btn active" onclick="switchTab('tab-query')">
            <svg width="18" height="18" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path></svg>
            Query & Spatial Explorer
        </button>
        <button class="tab-btn" onclick="switchTab('tab-curl')">
            <svg width="18" height="18" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"></path></svg>
            cURL Studio & Create
        </button>
        <button class="tab-btn" onclick="switchTab('tab-temporal')">
            <svg width="18" height="18" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"></path></svg>
            Temporal Analytics
        </button>
        <button class="tab-btn" onclick="switchTab('tab-dist')">
            <svg width="18" height="18" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"></path></svg>
            CSR & Subscriptions
        </button>
    </div>

    <!-- Main Content -->
    <main>
        <!-- TAB 1: QUERY & SPATIAL EXPLORER -->
        <div id="tab-query" class="tab-panel active">
            <div class="grid-split">
                <!-- Left: Query Parameters -->
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Query Builder (GET /ngsi-ld/v1/entities)</h2>
                            <button class="btn btn-primary btn-sm" onclick="runEntityQuery()">
                                Esegui Query
                            </button>
                        </div>

                        <div class="form-group">
                            <label class="form-label">Entity Type</label>
                            <input type="text" id="query-type" class="form-input" placeholder="Es. Vehicle, Building, Sensor (vuoto per tutti)">
                        </div>

                        <div class="form-group">
                            <label class="form-label">Filtro q (ETSI Query Language)</label>
                            <input type="text" id="query-q" class="form-input code" placeholder="Es. speed>80;brand=='Mercedes' oppure temperature>20">
                            <div style="font-size: 0.75rem; color: var(--text-muted); margin-top: 0.3rem;">
                                Supporta ==, !=, &gt;, &lt;, &gt;=, &lt;=, ~=, intervalli [min,max], ; (AND), | (OR).
                            </div>
                        </div>

                        <div class="form-group" style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">
                            <div>
                                <label class="form-label">Format Representation</label>
                                <select id="query-format" class="form-select">
                                    <option value="keyValues" selected>keyValues (Semplificato)</option>
                                    <option value="normalized">normalized (Completo)</option>
                                </select>
                            </div>
                            <div>
                                <label class="form-label">Limit Risultati</label>
                                <input type="number" id="query-limit" class="form-input" value="50" min="1" max="1000">
                            </div>
                        </div>

                        <!-- GeoQ Spatial Query Section -->
                        <div class="form-group" style="border-top: 1px solid var(--border); padding-top: 1rem; margin-top: 1rem;">
                            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.5rem;">
                                <label class="form-label" style="margin-bottom: 0;">Filtro Geografico (geoQ PostGIS)</label>
                                <input type="checkbox" id="enable-geoq" onchange="toggleGeoQInputs()">
                            </div>
                            <div id="geoq-inputs" style="display: none; grid-template-columns: 1fr 1fr; gap: 0.75rem; margin-top: 0.5rem;">
                                <div>
                                    <label class="form-label">Relazione Spaziale (georel)</label>
                                    <select id="geoq-rel" class="form-select">
                                        <option value="near;maxDistance==5000">near (Raggio Metri)</option>
                                        <option value="within">within (Dentro Poligono)</option>
                                        <option value="intersects">intersects</option>
                                    </select>
                                </div>
                                <div>
                                    <label class="form-label">Raggio Max (metri)</label>
                                    <input type="number" id="geoq-dist" class="form-input" value="5000">
                                </div>
                                <div style="grid-column: span 2;">
                                    <label class="form-label">Coordinate Centro (Lon, Lat)</label>
                                    <input type="text" id="geoq-coords" class="form-input code" value="[13.4050, 52.5200]">
                                </div>
                            </div>
                        </div>

                        <!-- Generated cURL box -->
                        <div class="code-container" style="margin-top: 1rem;">
                            <div class="code-header">
                                <span>cURL Equivalente</span>
                                <button class="btn btn-secondary btn-sm" onclick="copyCurl('query-curl-text')">Copia cURL</button>
                            </div>
                            <pre id="query-curl-text" style="max-height: 120px; font-size: 0.75rem;">curl http://localhost:8080/ngsi-ld/v1/entities</pre>
                        </div>
                    </div>

                    <!-- Spatial Map Visualizer -->
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Mappa Spaziale (GeoProperty)</h2>
                            <span id="map-entities-count" style="font-size: 0.8rem; color: var(--text-muted);">0 entità geolocalizzate</span>
                        </div>
                        <div id="map"></div>
                    </div>
                </div>

                <!-- Right: Query Results -->
                <div>
                    <div class="card" style="height: 100%; display: flex; flex-direction: column;">
                        <div class="card-header">
                            <h2 class="card-title">
                                Risultati Entità
                                <span id="results-count" class="badge" style="margin-left: 0.5rem;">0</span>
                            </h2>
                            <div style="display: flex; gap: 0.5rem;">
                                <button class="btn btn-secondary btn-sm" onclick="toggleResultView('cards')">Cards</button>
                                <button class="btn btn-secondary btn-sm" onclick="toggleResultView('json')">JSON Raw</button>
                                <button class="btn btn-secondary btn-sm" onclick="copyCurl('json-results-pre')">Copia JSON</button>
                            </div>
                        </div>

                        <!-- Cards View -->
                        <div id="results-cards-view" style="flex: 1; overflow-y: auto; max-height: 780px;">
                            <div style="text-align: center; color: var(--text-muted); padding: 3rem;">
                                Nessun dato caricato. Clicca su "Esegui Query" per caricare le entità.
                            </div>
                        </div>

                        <!-- JSON View -->
                        <div id="results-json-view" class="code-container" style="display: none; flex: 1;">
                            <pre id="json-results-pre" style="max-height: 780px;">[]</pre>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- TAB 2: CURL STUDIO & CREATOR -->
        <div id="tab-curl" class="tab-panel">
            <div class="grid-split">
                <!-- Left: Quick Ingestion Templates -->
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Template Rapidi di Creazione Entità</h2>
                        </div>
                        <p style="font-size: 0.85rem; color: var(--text-muted); margin-bottom: 1rem;">
                            Seleziona un modello NGSI-LD per caricare il payload e generare il comando cURL per il tuo terminale:
                        </p>
                        <div class="chip-group">
                            <button class="chip active" onclick="loadTemplate('vehicle')">🚗 Vehicle (Veicolo Connesso)</button>
                            <button class="chip" onclick="loadTemplate('weather')">⛅ WeatherObserved (Meteo)</button>
                            <button class="chip" onclick="loadTemplate('building')">🏢 Building (Smart City)</button>
                            <button class="chip" onclick="loadTemplate('sensor')">📡 Sensor (IoT)</button>
                        </div>

                        <div class="form-group">
                            <label class="form-label">Metodo & Endpoint</label>
                            <div style="display: flex; gap: 0.5rem;">
                                <select id="create-method" class="form-select" style="width: 130px;">
                                    <option value="POST">POST</option>
                                    <option value="PATCH">PATCH</option>
                                    <option value="DELETE">DELETE</option>
                                </select>
                                <input type="text" id="create-endpoint" class="form-input code" value="/ngsi-ld/v1/entities">
                            </div>
                        </div>

                        <div class="form-group">
                            <label class="form-label">JSON-LD Payload</label>
                            <textarea id="create-payload" class="form-textarea code" rows="16"></textarea>
                        </div>

                        <div style="display: flex; gap: 0.75rem;">
                            <button class="btn btn-primary" onclick="sendCreateRequest()">
                                Invia Richiesta al Broker
                            </button>
                            <button class="btn btn-secondary" onclick="updateCreateCurl(); copyCurl('create-curl-text')">
                                Copia cURL per Terminale
                            </button>
                        </div>
                    </div>
                </div>

                <!-- Right: Generated cURL & Live Response -->
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">cURL Pronto per il Terminale</h2>
                            <button class="btn btn-secondary btn-sm" onclick="copyCurl('create-curl-text')">Copia</button>
                        </div>
                        <div class="code-container">
                            <pre id="create-curl-text" style="max-height: 220px; font-size: 0.8rem;"></pre>
                        </div>
                    </div>

                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Risposta Live dal Server</h2>
                            <span id="create-status-code" class="badge" style="display: none;"></span>
                        </div>
                        <div class="code-container">
                            <pre id="create-response-text" style="max-height: 380px; font-size: 0.85rem;">In attesa di esecuzione...</pre>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- TAB 3: TEMPORAL ANALYTICS -->
        <div id="tab-temporal" class="tab-panel">
            <div class="grid-split">
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Query Serie Storiche (/temporal/entities)</h2>
                            <button class="btn btn-primary btn-sm" onclick="runTemporalQuery()">Interroga</button>
                        </div>

                        <div class="form-group">
                            <label class="form-label">Seleziona Entità da quelle attive</label>
                            <select id="temp-entity-select" class="form-select" onchange="onTemporalEntitySelect(this.value)">
                                <option value="">-- Seleziona entità o digita manualmente sotto --</option>
                            </select>
                        </div>

                        <div class="form-group">
                            <label class="form-label">ID Entità NGSI-LD</label>
                            <input type="text" id="temp-entity-id" class="form-input code" value="urn:ngsi-ld:Sensor:Device01">
                        </div>

                        <div class="form-group" style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">
                            <div>
                                <label class="form-label">Relazione Temporale (timerel)</label>
                                <select id="temp-timerel" class="form-select" onchange="toggleEndTimeInput()">
                                    <option value="after" selected>after</option>
                                    <option value="between">between</option>
                                    <option value="before">before</option>
                                </select>
                            </div>
                            <div>
                                <label class="form-label">Attributo</label>
                                <input type="text" id="temp-attr" class="form-input code" value="co2">
                            </div>
                        </div>

                        <div class="form-group" style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">
                            <div>
                                <label class="form-label">Inizio (timeAt)</label>
                                <input type="text" id="temp-timeat" class="form-input code" value="2026-09-20T00:00:00Z">
                            </div>
                            <div id="endtime-container" style="display: none;">
                                <label class="form-label">Fine (endTimeAt)</label>
                                <input type="text" id="temp-endtimeat" class="form-input code" value="2026-09-22T23:59:59Z">
                            </div>
                        </div>

                        <div style="display: flex; gap: 0.5rem; margin-bottom: 1rem;">
                            <button type="button" class="btn btn-secondary btn-sm" onclick="setTimePreset('24h')">Ultime 24h</button>
                            <button type="button" class="btn btn-secondary btn-sm" onclick="setTimePreset('7d')">Ultimi 7gg</button>
                            <button type="button" class="btn btn-secondary btn-sm" onclick="setTimePreset('all')">Tutto lo storico</button>
                        </div>

                        <div class="form-group" style="display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;">
                            <div>
                                <label class="form-label">Time Property</label>
                                <select id="temp-timeproperty" class="form-select">
                                    <option value="observedAt" selected>observedAt</option>
                                    <option value="createdAt">createdAt</option>
                                    <option value="modifiedAt">modifiedAt</option>
                                </select>
                            </div>
                            <div>
                                <label class="form-label">Limit punti recenti (lastN)</label>
                                <input type="number" id="temp-lastn" class="form-input" placeholder="Es. 10 (vuoto = tutti)" min="1" max="1000">
                            </div>
                        </div>

                        <div class="form-group">
                            <label class="form-label">Metodo di Aggregazione (aggrMethod)</label>
                            <select id="temp-aggr" class="form-select">
                                <option value="">Nessuna aggregazione (tutti i punti)</option>
                                <option value="avg">avg (Media)</option>
                                <option value="max">max (Massimo)</option>
                                <option value="min">min (Minimo)</option>
                                <option value="sum">sum (Somma)</option>
                                <option value="totalCount">totalCount (Conteggio)</option>
                                <option value="distinctCount">distinctCount (Conteggio univoci)</option>
                            </select>
                        </div>

                        <div class="code-container" style="margin-top: 1rem;">
                            <div class="code-header">
                                <span>cURL Temporale</span>
                                <button class="btn btn-secondary btn-sm" onclick="copyCurl('temp-curl-text')">Copia</button>
                            </div>
                            <pre id="temp-curl-text" style="max-height: 100px; font-size: 0.75rem;"></pre>
                        </div>
                    </div>
                </div>


                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Grafico di Evoluzione Temporale</h2>
                            <div id="temporal-stats-badges" style="display: flex; gap: 0.5rem; flex-wrap: wrap;"></div>
                        </div>

                        <div style="height: 350px; position: relative;">
                            <canvas id="temporalChart"></canvas>
                        </div>
                    </div>

                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Payload Risultato Temporale</h2>
                        </div>
                        <div class="code-container">
                            <pre id="temp-result-json" style="max-height: 250px;">[]</pre>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- TAB 4: CSR & SUBSCRIPTIONS -->
        <div id="tab-dist" class="tab-panel">
            <div class="grid-split">
                <!-- Context Source Registrations -->
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Context Source Registrations (CSR)</h2>
                            <button class="btn btn-secondary btn-sm" onclick="loadCsrList()">Aggiorna</button>
                        </div>
                        <p style="font-size: 0.85rem; color: var(--text-muted); margin-bottom: 1rem;">
                            Sorgenti di contesto distribuite federate da Athena Broker per le query remote (ETSI cl. 5.9):
                        </p>
                        <div id="csr-list" style="max-height: 600px; overflow-y: auto;">
                            <div style="text-align: center; color: var(--text-muted); padding: 2rem;">Caricamento in corso...</div>
                        </div>
                    </div>
                </div>

                <!-- Subscriptions -->
                <div>
                    <div class="card">
                        <div class="card-header">
                            <h2 class="card-title">Sottoscrizioni Attive</h2>
                            <button class="btn btn-secondary btn-sm" onclick="loadSubscriptionsList()">Aggiorna</button>
                        </div>
                        <p style="font-size: 0.85rem; color: var(--text-muted); margin-bottom: 1rem;">
                            Eventi e notifiche asincrone con worker Tokio non-bloccanti:
                        </p>
                        <div id="sub-list" style="max-height: 600px; overflow-y: auto;">
                            <div style="text-align: center; color: var(--text-muted); padding: 2rem;">Caricamento in corso...</div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </main>

    <!-- Toast Notification -->
    <div id="toast">Copiato negli appunti!</div>

    <script>
        // State
        let currentTab = 'tab-query';
        let leafletMap = null;
        let mapMarkers = [];
        let temporalChartInstance = null;
        let lastEntities = [];

        // Templates
        const templates = {
            vehicle: {
                id: "urn:ngsi-ld:Vehicle:A102",
                type: "Vehicle",
                brand: { type: "Property", value: "Mercedes" },
                speed: { type: "Property", value: 85.5, unitCode: "KMH" },
                location: {
                    type: "GeoProperty",
                    value: { type: "Point", coordinates: [13.4050, 52.5200] }
                },
                "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
            },
            weather: {
                id: "urn:ngsi-ld:WeatherObserved:Roma01",
                type: "WeatherObserved",
                temperature: { type: "Property", value: 24.2, unitCode: "CEL" },
                humidity: { type: "Property", value: 65.0 },
                location: {
                    type: "GeoProperty",
                    value: { type: "Point", coordinates: [12.4964, 41.9028] }
                },
                "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
            },
            building: {
                id: "urn:ngsi-ld:Building:MilanoHQ",
                type: "Building",
                name: { type: "Property", value: "Sede Centrale Milano" },
                floors: { type: "Property", value: 12 },
                location: {
                    type: "GeoProperty",
                    value: { type: "Point", coordinates: [9.1900, 45.4642] }
                },
                "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
            },
            sensor: {
                id: "urn:ngsi-ld:Sensor:AirQuality01",
                type: "Sensor",
                pm10: { type: "Property", value: 18.4, unitCode: "GQ" },
                co2: { type: "Property", value: 412.0, unitCode: "59" },
                location: {
                    type: "GeoProperty",
                    value: { type: "Point", coordinates: [14.2681, 40.8518] }
                },
                "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
            }
        };

        // Initialize App
        window.addEventListener('DOMContentLoaded', () => {
            initMap();
            loadTemplate('vehicle');
            checkBrokerHealth();
            runEntityQuery();
        });

        function switchTab(tabId) {
            document.querySelectorAll('.tab-btn').forEach(btn => btn.classList.remove('active'));
            document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));

            event.currentTarget.classList.add('active');
            document.getElementById(tabId).classList.add('active');
            currentTab = tabId;

            if (tabId === 'tab-query' && leafletMap) {
                setTimeout(() => leafletMap.invalidateSize(), 200);
            }
            if (tabId === 'tab-dist') {
                loadCsrList();
                loadSubscriptionsList();
            }
        }

        function initMap() {
            leafletMap = L.map('map').setView([48.0, 12.0], 4);
            L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
                attribution: '© OpenStreetMap contributors',
                maxZoom: 18
            }).addTo(leafletMap);
        }

        function toggleGeoQInputs() {
            const enabled = document.getElementById('enable-geoq').checked;
            document.getElementById('geoq-inputs').style.display = enabled ? 'grid' : 'none';
            updateQueryCurl();
        }

        function loadTemplate(name) {
            document.querySelectorAll('.chip').forEach(c => c.classList.remove('active'));
            event && event.target && event.target.classList.add('active');

            const payload = templates[name];
            document.getElementById('create-payload').value = JSON.stringify(payload, null, 2);
            document.getElementById('create-method').value = "POST";
            document.getElementById('create-endpoint').value = "/ngsi-ld/v1/entities";
            updateCreateCurl();
        }

        function showToast(msg) {
            const t = document.getElementById('toast');
            t.innerText = msg;
            t.classList.add('show');
            setTimeout(() => t.classList.remove('show'), 2500);
        }

        function copyCurl(elementId) {
            const text = document.getElementById(elementId).innerText;
            navigator.clipboard.writeText(text).then(() => {
                showToast("Copiato negli appunti!");
            });
        }

        async function checkBrokerHealth() {
            try {
                const res = await fetch('/health');
                if (res.ok) {
                    const data = await res.json();
                    document.getElementById('broker-status-text').innerText = `${data.status} (v${data.version})`;
                }
            } catch (e) {
                document.getElementById('broker-status-text').innerText = 'Offline';
            }
        }

        // -------------------------------------------------------------
        // ENTITY QUERY LOGIC
        // -------------------------------------------------------------
        function buildQueryUrl() {
            const type = document.getElementById('query-type').value.trim();
            const q = document.getElementById('query-q').value.trim();
            const format = document.getElementById('query-format').value;
            const limit = document.getElementById('query-limit').value;
            const geoqEnabled = document.getElementById('enable-geoq').checked;

            const params = new URLSearchParams();
            if (type) params.append('type', type);
            if (q) params.append('q', q);
            if (format) params.append('options', format);
            if (limit) params.append('limit', limit);

            if (geoqEnabled) {
                params.append('georel', document.getElementById('geoq-rel').value);
                params.append('geometry', 'Point');
                params.append('coordinates', document.getElementById('geoq-coords').value);
            }

            return `/ngsi-ld/v1/entities?${params.toString()}`;
        }

        function updateQueryCurl() {
            const url = buildQueryUrl();
            const curl = `curl -s -G "http://localhost:8080${url.split('?')[0]}" \\\n  ${url.split('?')[1].split('&').map(p => {
                const [k, v] = p.split('=');
                return `--data-urlencode "${k}=${decodeURIComponent(v)}"`;
            }).join(' \\\n  ')} \\\n  -H "Accept: application/ld+json"`;
            document.getElementById('query-curl-text').innerText = curl;
        }

        document.getElementById('query-type').addEventListener('input', updateQueryCurl);
        document.getElementById('query-q').addEventListener('input', updateQueryCurl);
        document.getElementById('query-format').addEventListener('change', updateQueryCurl);
        document.getElementById('query-limit').addEventListener('input', updateQueryCurl);

        async function runEntityQuery() {
            updateQueryCurl();
            const url = buildQueryUrl();
            try {
                const res = await fetch(url, { headers: { 'Accept': 'application/ld+json' } });
                const data = await res.json();
                lastEntities = Array.isArray(data) ? data : (data ? [data] : []);

                document.getElementById('results-count').innerText = lastEntities.length;
                document.getElementById('entity-count-badge').innerText = `${lastEntities.length} Entità`;
                document.getElementById('json-results-pre').innerText = JSON.stringify(lastEntities, null, 2);

                renderEntityCards(lastEntities);
                updateMapMarkers(lastEntities);
                updateTemporalEntityDropdown(lastEntities);
            } catch (e) {
                document.getElementById('results-cards-view').innerHTML = `<div style="color: var(--danger); padding: 2rem;">Errore di connessione: ${e.message}</div>`;
            }
        }

        function renderEntityCards(entities) {
            const container = document.getElementById('results-cards-view');
            if (!entities || entities.length === 0) {
                container.innerHTML = `<div style="text-align: center; color: var(--text-muted); padding: 3rem;">Nessuna entità trovata con i filtri correnti.</div>`;
                return;
            }

            container.innerHTML = entities.map(e => {
                let propsHtml = '';
                let firstDataAttr = '';
                for (const [k, v] of Object.entries(e)) {
                    if (k === 'id' || k === 'type' || k === '@context') continue;
                    let displayVal = typeof v === 'object' && v !== null && v.value !== undefined ? v.value : JSON.stringify(v);
                    propsHtml += `<span class="prop-tag">${k}: <strong>${displayVal}</strong></span>`;
                    if (!firstDataAttr && k !== 'location') {
                        firstDataAttr = k;
                    }
                }

                const temporalBtn = firstDataAttr ? `
                    <button class="btn btn-secondary btn-sm" onclick="openTemporalForEntity('${e.id}', '${firstDataAttr}')" style="margin-top: 0.75rem; font-size: 0.75rem; padding: 0.25rem 0.6rem;">
                        📈 Analisi Storica (${firstDataAttr})
                    </button>
                ` : '';

                return `
                    <div class="entity-card">
                        <div class="entity-header">
                            <span class="entity-id">${e.id}</span>
                            <span class="entity-type">${e.type}</span>
                        </div>
                        <div class="entity-props">${propsHtml}</div>
                        <div style="display: flex; justify-content: flex-end;">${temporalBtn}</div>
                    </div>
                `;
            }).join('');
        }


        function updateMapMarkers(entities) {
            if (!leafletMap) return;
            mapMarkers.forEach(m => leafletMap.removeLayer(m));
            mapMarkers = [];

            let bounds = [];
            entities.forEach(e => {
                let coords = null;
                if (e.location && e.location.coordinates) {
                    coords = e.location.coordinates;
                } else if (e.location && e.location.value && e.location.value.coordinates) {
                    coords = e.location.value.coordinates;
                }

                if (coords && coords.length === 2) {
                    const [lon, lat] = coords;
                    const marker = L.marker([lat, lon]).addTo(leafletMap);
                    marker.bindPopup(`<b>${e.id}</b><br>Type: ${e.type}`);
                    mapMarkers.push(marker);
                    bounds.push([lat, lon]);
                }
            });

            document.getElementById('map-entities-count').innerText = `${mapMarkers.length} entità geolocalizzate`;
            if (bounds.length > 0) {
                leafletMap.fitBounds(bounds, { padding: [40, 40], maxZoom: 12 });
            }
        }

        function toggleResultView(mode) {
            if (mode === 'cards') {
                document.getElementById('results-cards-view').style.display = 'block';
                document.getElementById('results-json-view').style.display = 'none';
            } else {
                document.getElementById('results-cards-view').style.display = 'none';
                document.getElementById('results-json-view').style.display = 'block';
            }
        }

        // -------------------------------------------------------------
        // CURL STUDIO LOGIC
        // -------------------------------------------------------------
        function updateCreateCurl() {
            const method = document.getElementById('create-method').value;
            const endpoint = document.getElementById('create-endpoint').value;
            const payload = document.getElementById('create-payload').value.trim();

            let curl = `curl -i -X ${method} "http://localhost:8080${endpoint}" \\\n  -H "Content-Type: application/ld+json"`;
            if (payload && method !== 'DELETE') {
                curl += ` \\\n  -d '${payload.replace(/'/g, "'\\''")}'`;
            }
            document.getElementById('create-curl-text').innerText = curl;
        }

        document.getElementById('create-payload').addEventListener('input', updateCreateCurl);
        document.getElementById('create-endpoint').addEventListener('input', updateCreateCurl);
        document.getElementById('create-method').addEventListener('change', updateCreateCurl);

        async function sendCreateRequest() {
            updateCreateCurl();
            const method = document.getElementById('create-method').value;
            const endpoint = document.getElementById('create-endpoint').value;
            const payload = document.getElementById('create-payload').value.trim();

            const options = {
                method,
                headers: { 'Content-Type': 'application/ld+json' }
            };
            if (payload && method !== 'DELETE') {
                options.body = payload;
            }

            try {
                const res = await fetch(endpoint, options);
                const badge = document.getElementById('create-status-code');
                badge.style.display = 'inline-block';
                badge.innerText = `HTTP ${res.status} ${res.statusText}`;
                badge.style.backgroundColor = res.ok ? 'rgba(16, 185, 129, 0.2)' : 'rgba(239, 68, 68, 0.2)';
                badge.style.color = res.ok ? '#34D399' : '#F87171';

                const text = await res.text();
                try {
                    document.getElementById('create-response-text').innerText = JSON.stringify(JSON.parse(text), null, 2);
                } catch {
                    document.getElementById('create-response-text').innerText = text || "(Risposta vuota con status " + res.status + ")";
                }

                showToast(`Richiesta completata: ${res.status}`);
                // Refresh entity query if we created an entity
                runEntityQuery();
            } catch (e) {
                document.getElementById('create-response-text').innerText = `Errore di connessione: ${e.message}`;
            }
        }

        // -------------------------------------------------------------
        // TEMPORAL LOGIC & CHART
        // -------------------------------------------------------------
        function toggleEndTimeInput() {
            const rel = document.getElementById('temp-timerel').value;
            const container = document.getElementById('endtime-container');
            if (container) {
                container.style.display = rel === 'between' ? 'block' : 'none';
            }
        }

        function setTimePreset(preset) {
            const now = new Date();
            let start = new Date();
            if (preset === '24h') {
                start.setHours(now.getHours() - 24);
                document.getElementById('temp-timerel').value = 'after';
            } else if (preset === '7d') {
                start.setDate(now.getDate() - 7);
                document.getElementById('temp-timerel').value = 'after';
            } else if (preset === 'all') {
                start = new Date('2020-01-01T00:00:00Z');
                document.getElementById('temp-timerel').value = 'after';
            }
            document.getElementById('temp-timeat').value = start.toISOString();
            document.getElementById('temp-endtimeat').value = now.toISOString();
            toggleEndTimeInput();
            runTemporalQuery();
        }

        function onTemporalEntitySelect(val) {
            if (!val) return;
            document.getElementById('temp-entity-id').value = val;
            const ent = lastEntities.find(e => e.id === val);
            if (ent) {
                for (const k of Object.keys(ent)) {
                    if (k !== 'id' && k !== 'type' && k !== '@context' && k !== 'location') {
                        document.getElementById('temp-attr').value = k;
                        break;
                    }
                }
            }
            runTemporalQuery();
        }

        function updateTemporalEntityDropdown(entities) {
            const sel = document.getElementById('temp-entity-select');
            if (!sel) return;
            const curVal = document.getElementById('temp-entity-id').value;
            sel.innerHTML = '<option value="">-- Seleziona entità attiva (' + entities.length + ' disponibili) --</option>' +
                entities.map(e => `<option value="${e.id}" ${e.id === curVal ? 'selected' : ''}>${e.id} (${e.type})</option>`).join('');
        }

        function openTemporalForEntity(entityId, attrName) {
            document.getElementById('temp-entity-id').value = entityId;
            if (attrName) document.getElementById('temp-attr').value = attrName;

            // Switch to tab-temporal
            document.querySelectorAll('.tab-btn').forEach(btn => btn.classList.remove('active'));
            document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
            const tabBtn = Array.from(document.querySelectorAll('.tab-btn')).find(b => b.innerText.includes('Temporal'));
            if (tabBtn) tabBtn.classList.add('active');
            document.getElementById('tab-temporal').classList.add('active');

            runTemporalQuery();
        }

        async function runTemporalQuery() {
            const id = document.getElementById('temp-entity-id').value.trim();
            const timerel = document.getElementById('temp-timerel').value;
            const attr = document.getElementById('temp-attr').value.trim();
            const timeAt = document.getElementById('temp-timeat').value.trim();
            const endTimeAt = document.getElementById('temp-endtimeat').value.trim();
            const aggr = document.getElementById('temp-aggr').value;
            const timeprop = document.getElementById('temp-timeproperty').value;
            const lastN = document.getElementById('temp-lastn').value.trim();

            const params = new URLSearchParams();
            params.append('timerel', timerel);
            params.append('timeAt', timeAt);
            if (timerel === 'between' && endTimeAt) params.append('endTimeAt', endTimeAt);
            if (attr) params.append('attrs', attr);
            if (aggr) params.append('aggrMethod', aggr);
            if (timeprop && timeprop !== 'observedAt') params.append('timeproperty', timeprop);
            if (lastN) params.append('lastN', lastN);

            const url = `/ngsi-ld/v1/temporal/entities/${encodeURIComponent(id)}?${params.toString()}`;

            const curl = `curl -s -G "http://localhost:8080/ngsi-ld/v1/temporal/entities/${id}" \\\n  ${params.toString().split('&').map(p => {
                const [k, v] = p.split('=');
                return `--data-urlencode "${k}=${decodeURIComponent(v)}"`;
            }).join(' \\\n  ')} \\\n  -H "Accept: application/ld+json"`;
            document.getElementById('temp-curl-text').innerText = curl;

            try {
                const res = await fetch(url, { headers: { 'Accept': 'application/ld+json' } });
                const data = await res.json();
                document.getElementById('temp-result-json').innerText = JSON.stringify(data, null, 2);

                renderTemporalChart(data, attr);
            } catch (e) {
                document.getElementById('temp-result-json').innerText = `Errore: ${e.message}`;
            }
        }

        function renderTemporalChart(data, attrName) {
            const ctx = document.getElementById('temporalChart').getContext('2d');
            let labels = [];
            let values = [];

            if (data && data[attrName]) {
                const instances = data[attrName];
                if (Array.isArray(instances)) {
                    instances.forEach(inst => {
                        const dateObj = new Date(inst.observedAt || inst.createdAt || inst.modifiedAt);
                        labels.push(dateObj.toLocaleTimeString());
                        values.push(typeof inst.value === 'number' ? inst.value : parseFloat(inst.value) || 0);
                    });
                } else if (instances.values && Array.isArray(instances.values)) {
                    instances.values.forEach(v => {
                        labels.push("Aggregato");
                        values.push(typeof v[0] === 'number' ? v[0] : parseFloat(v[0]) || 0);
                    });
                }
            }

            // Update stats badges
            const badgesContainer = document.getElementById('temporal-stats-badges');
            if (badgesContainer && values.length > 0) {
                const count = values.length;
                const min = Math.min(...values);
                const max = Math.max(...values);
                const sum = values.reduce((a, b) => a + b, 0);
                const avg = (sum / count).toFixed(2);
                const last = values[values.length - 1];
                badgesContainer.innerHTML = `
                    <span class="badge" style="background: rgba(59,130,246,0.2); color: #93C5FD;">Punti: <strong>${count}</strong></span>
                    <span class="badge" style="background: rgba(16,185,129,0.2); color: #6EE7B7;">Min: <strong>${min}</strong></span>
                    <span class="badge" style="background: rgba(239,68,68,0.2); color: #FCA5A5;">Max: <strong>${max}</strong></span>
                    <span class="badge" style="background: rgba(245,158,11,0.2); color: #FCD34D;">Media: <strong>${avg}</strong></span>
                    <span class="badge" style="background: rgba(139,92,246,0.2); color: #DDD6FE;">Ultimo: <strong>${last}</strong></span>
                `;
            } else if (badgesContainer) {
                badgesContainer.innerHTML = '';
            }

            if (temporalChartInstance) {
                temporalChartInstance.destroy();
            }

            temporalChartInstance = new Chart(ctx, {
                type: 'line',
                data: {
                    labels: labels.length ? labels : ['Nessun dato'],
                    datasets: [{
                        label: attrName || 'Valore',
                        data: values.length ? values : [0],
                        borderColor: '#3B82F6',
                        backgroundColor: 'rgba(59, 130, 246, 0.1)',
                        fill: true,
                        tension: 0.3,
                        pointRadius: 6,
                        pointHoverRadius: 8
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    scales: {
                        x: { grid: { color: '#374151' }, ticks: { color: '#9CA3AF' } },
                        y: { grid: { color: '#374151' }, ticks: { color: '#9CA3AF' } }
                    },
                    plugins: {
                        legend: { labels: { color: '#F9FAFB' } }
                    }
                }
            });
        }


        // -------------------------------------------------------------
        // CSR & SUBSCRIPTIONS LISTING
        // -------------------------------------------------------------
        async function loadCsrList() {
            try {
                const res = await fetch('/ngsi-ld/v1/csourceRegistrations');
                const list = await res.json();
                const container = document.getElementById('csr-list');
                if (!Array.isArray(list) || list.length === 0) {
                    container.innerHTML = `<div style="text-align: center; color: var(--text-muted); padding: 2rem;">Nessuna Context Source registrata.</div>`;
                    return;
                }
                container.innerHTML = list.map(c => `
                    <div class="entity-card">
                        <div class="entity-header">
                            <span class="entity-id">${c.id}</span>
                            <span class="badge">${c.status || 'active'}</span>
                        </div>
                        <div style="font-size: 0.85rem; color: #93C5FD; margin: 0.2rem 0;">${c.endpoint}</div>
                        <div class="entity-props">
                            <span class="prop-tag">Nome: <strong>${c.registrationName || '-'}</strong></span>
                        </div>
                    </div>
                `).join('');
            } catch (e) {
                document.getElementById('csr-list').innerText = e.message;
            }
        }

        async function loadSubscriptionsList() {
            try {
                const res = await fetch('/ngsi-ld/v1/subscriptions');
                const list = await res.json();
                const container = document.getElementById('sub-list');
                if (!Array.isArray(list) || list.length === 0) {
                    container.innerHTML = `<div style="text-align: center; color: var(--text-muted); padding: 2rem;">Nessuna sottoscrizione attiva.</div>`;
                    return;
                }
                container.innerHTML = list.map(s => `
                    <div class="entity-card">
                        <div class="entity-header">
                            <span class="entity-id">${s.id}</span>
                            <span class="badge">${s.status || 'active'}</span>
                        </div>
                        <div class="entity-props">
                            <span class="prop-tag">Watched: <strong>${(s.watchedAttributes || []).join(', ') || 'Tutti'}</strong></span>
                            <span class="prop-tag">q: <strong>${s.q || '-'}</strong></span>
                        </div>
                    </div>
                `).join('');
            } catch (e) {
                document.getElementById('sub-list').innerText = e.message;
            }
        }
    </script>
</body>
</html>
"#;
