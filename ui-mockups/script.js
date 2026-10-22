document.addEventListener("DOMContentLoaded", () => {
    // Sidebar Toggle Logic
    const sidebar = document.querySelector(".sidebar");
    const sidebarToggle = document.getElementById("sidebar-toggle");

    // Check saved state
    const isCollapsed = localStorage.getItem("sidebar-collapsed") === "true";
    if (isCollapsed) {
        sidebar.classList.add("collapsed");
    }

    sidebarToggle.addEventListener("click", () => {
        sidebar.classList.toggle("collapsed");
        localStorage.setItem(
            "sidebar-collapsed",
            sidebar.classList.contains("collapsed"),
        );
    });

    // Navigation Logic
    const navLinks = document.querySelectorAll(".nav-links .nav-item");
    const sections = document.querySelectorAll(".view-section");

    navLinks.forEach((link) => {
        link.addEventListener("click", (e) => {
            e.preventDefault();
            navLinks.forEach((l) => l.classList.remove("active"));
            sections.forEach((s) => s.classList.remove("active"));
            link.classList.add("active");
            const targetId = link.getAttribute("data-target") + "-view";
            const targetSection = document.getElementById(targetId);
            if (targetSection) targetSection.classList.add("active");
        });
    });

    // Settings Tab Logic
    const settingTabBtns = document.querySelectorAll(".s-tab-btn");
    const settingTabContents = document.querySelectorAll(".s-tab-content");

    settingTabBtns.forEach((btn) => {
        btn.addEventListener("click", () => {
            const targetTab = btn.getAttribute("data-tab");

            // Update buttons
            settingTabBtns.forEach((b) => b.classList.remove("active"));
            btn.classList.add("active");

            // Update contents
            settingTabContents.forEach((content) => {
                content.classList.remove("active");
                if (content.id === targetTab) {
                    content.classList.add("active");
                }
            });
        });
    });

    // State management
    const currentState = {
        selectedNodeId: "us-01", // Default
        startTime: null,
        isConnected: false,
    };

    // Helpers
    function getFlagUrl(countryCode) {
        if (!countryCode) return "";
        return `https://flagcdn.com/w80/${countryCode.toLowerCase()}.png`;
    }

    // Strict Route Sync Logic
    const dashStrict = document.getElementById("dash-strict-route");
    const settingsStrict = document.getElementById("tun-strict-route");

    if (dashStrict && settingsStrict) {
        dashStrict.addEventListener("change", (e) => {
            if (settingsStrict) settingsStrict.checked = e.target.checked;
        });
        settingsStrict.addEventListener("change", (e) => {
            if (dashStrict) dashStrict.checked = e.target.checked;
        });
    }

    // Auto Route Sync Logic
    const dashAuto = document.getElementById("dash-auto-route");
    const settingsAuto = document.getElementById("tun-auto-route");

    if (dashAuto && settingsAuto) {
        dashAuto.addEventListener("change", (e) => {
            if (settingsAuto) settingsAuto.checked = e.target.checked;
        });
        settingsAuto.addEventListener("change", (e) => {
            if (dashAuto) dashAuto.checked = e.target.checked;
        });
    }

    // Allow LAN Logic
    // Mixed Port Display Sync
    const mixedPortInput = document.getElementById("inbound-mixed-port");
    const mixedPortDisplay = document.getElementById("mixed-port-display");
    if (mixedPortInput && mixedPortDisplay) {
        mixedPortInput.addEventListener("input", (e) => {
            mixedPortDisplay.textContent = e.target.value;
        });
    }

    // DNS FakeIP Toggle Logic
    const fakeIpToggle = document.getElementById("dns-fakeip");
    const fakeIpExtras = document.getElementById("fakeip-extras");

    if (fakeIpToggle && fakeIpExtras) {
        fakeIpToggle.addEventListener("change", (e) => {
            fakeIpExtras.style.display = e.target.checked ? "flex" : "none";
        });
        // Initial state
        fakeIpExtras.style.display = fakeIpToggle.checked ? "flex" : "none";
    }

    /* --- PROXIES LOGIC --- */
    const nodeGrid = document.getElementById("node-grid");
    let hideTimeout = false;

    // Initial Render
    renderNodes();

    // Listeners for Proxies Toolbar
    const hideTimeoutToggle = document.getElementById("hide-timeout-toggle");
    const testAllBtn = document.getElementById("test-all-btn");

    if (hideTimeoutToggle) {
        hideTimeoutToggle.addEventListener("change", (e) => {
            hideTimeout = e.target.checked;
            renderNodes();
        });
    }

    if (testAllBtn) {
        testAllBtn.addEventListener("click", async (e) => {
            testAllNodes(testAllBtn);
        });
    }

    const speedTestAllBtn = document.getElementById("speed-test-all-btn");
    if (speedTestAllBtn) {
        speedTestAllBtn.addEventListener("click", () => {
            testAllNodesSpeed(speedTestAllBtn);
        });
    }

    async function testAllNodesSpeed(btn = null) {
        let originalText = "";
        if (btn) {
            originalText = btn.innerHTML;
            btn.disabled = true;
            btn.innerHTML =
                '<i class="fa-solid fa-spinner fa-spin"></i> Testing Speed...';
        }

        const promises = mockNodes.map((node) => {
            // Find the individual speed test button for this node
            const speedBtn = document.querySelector(
                `.node-card[data-id="${node.id}"] .node-speed-test-btn`,
            ) || {
                // fallback if card not found or already testing
                disabled: false,
                innerHTML: '<i class="fa-solid fa-gauge-high"></i>',
            };
            return testNodeSpeed(node, speedBtn);
        });

        await Promise.all(promises);

        if (btn) {
            btn.disabled = false;
            btn.innerHTML = originalText;
        }
    }

    async function testAllNodes(btn = null) {
        let originalText = "";
        if (btn) {
            originalText = btn.innerHTML;
            btn.disabled = true;
            btn.innerHTML =
                '<i class="fa-solid fa-spinner fa-spin"></i> Testing...';
        }

        const promises = mockNodes.map((node) => testNodeLatency(node));
        await Promise.all(promises);

        if (btn) {
            btn.disabled = false;
            btn.innerHTML = originalText;
        }
        renderNodes(); // Final render to apply hideTimeout filter if needed
    }

    async function testNodeLatency(node) {
        const latencyEl = document.getElementById(`latency-${node.id}`);
        if (latencyEl) {
            latencyEl.innerHTML = '<i class="fa-solid fa-spinner fa-spin"></i>';
            latencyEl.className = "node-latency";
        }

        // Simulate network delay
        await new Promise((resolve) =>
            setTimeout(resolve, 500 + Math.random() * 1000),
        );

        // 15% chance of timeout
        const isTimeout = Math.random() > 0.85;
        node.latency = isTimeout ? -1 : Math.floor(Math.random() * 200 + 20);

        updateNodeCardLatency(node);
    }

    function updateNodeCardLatency(node) {
        const latencyEl = document.getElementById(`latency-${node.id}`);
        if (!latencyEl) return;

        if (node.latency === -1) {
            latencyEl.textContent = "Timeout";
            latencyEl.className = "node-latency bad";
        } else {
            latencyEl.textContent = `${node.latency} ms`;
            latencyEl.className = `node-latency ${node.latency < 100 ? "good" : node.latency < 250 ? "medium" : "bad"}`;
        }
    }

    async function testNodeSpeed(node, btn) {
        if (!btn) return;
        const originalHtml = btn.innerHTML;
        btn.disabled = true;
        btn.innerHTML = '<i class="fa-solid fa-spinner fa-spin"></i>';

        const upEl = document.getElementById(`speed-up-${node.id}`);
        const downEl = document.getElementById(`speed-down-${node.id}`);

        // Simulation
        let ticks = 0;
        const interval = setInterval(() => {
            node.speed.up = (Math.random() * 5 + 1).toFixed(1);
            node.speed.down = (Math.random() * 50 + 10).toFixed(1);
            if (upEl) upEl.textContent = `${node.speed.up} MB/s`;
            if (downEl) downEl.textContent = `${node.speed.down} MB/s`;
            ticks++;
            if (ticks > 15) {
                clearInterval(interval);
                btn.disabled = false;
                btn.innerHTML = originalHtml;
            }
        }, 100);
    }

    function renderNodes() {
        if (!nodeGrid) return;
        nodeGrid.innerHTML = "";
        mockNodes.forEach((node) => {
            // Hide Timeout Filter
            if (hideTimeout && node.latency === -1) return;

            const card = document.createElement("div");
            card.className = "node-card";
            card.setAttribute("data-id", node.id);
            if (node.id === currentState.selectedNodeId)
                card.classList.add("active");

            let latencyClass = "";
            let latencyText = "-- ms";

            if (node.latency === -1) {
                latencyClass = "bad";
                latencyText = "Timeout";
            } else if (node.latency) {
                latencyClass =
                    node.latency < 100
                        ? "good"
                        : node.latency < 250
                          ? "medium"
                          : "bad";
                latencyText = `${node.latency} ms`;
            }

            card.innerHTML = `
                <div class="node-header">
                    <div style="display: flex; align-items: center; gap: 8px;">
                        <img src="${getFlagUrl(node.country_code)}" class="node-flag-img" alt="${node.country_code}">
                        <div class="country-badge">${node.country_code}</div>
                    </div>
                    <div class="node-latency-wrapper">
                        <div class="node-latency ${latencyClass}" id="latency-${node.id}">${latencyText}</div>
                        <button class="node-refresh-btn" data-id="${node.id}" title="Test Latency">
                            <i class="fa-solid fa-rotate"></i>
                        </button>
                    </div>
                </div>
                <div class="node-info">
                    <h3>${node.name}</h3>
                    <div class="node-meta-grid">
                        <div class="meta-item"><i class="fa-solid fa-shield-halved"></i> ${node.protocol}</div>
                        <div class="meta-item ${node.transport === "UDP" ? (node.udp_over_tcp ? "udp-tunneled" : "udp-native") : ""}">
                            <i class="fa-solid fa-network-wired"></i> 
                            ${node.transport} ${node.transport === "UDP" ? (node.udp_over_tcp ? "(TCP)" : "") : ""}
                        </div>
                        <div class="meta-item"><i class="fa-solid fa-certificate"></i> ${node.tls}</div>
                        <div class="meta-item"><i class="fa-solid fa-location-dot"></i> ${node.country_name}</div>
                    </div>
                </div>
                <div class="node-speed-area">
                    <div class="speed-metrics">
                        <div class="speed-item">
                            <i class="fa-solid fa-arrow-down"></i>
                            <span id="speed-down-${node.id}">${node.speed.down > 0 ? node.speed.down + " MB/s" : "-- MB/s"}</span>
                        </div>
                        <div class="speed-item">
                            <i class="fa-solid fa-arrow-up"></i>
                            <span id="speed-up-${node.id}">${node.speed.up > 0 ? node.speed.up + " MB/s" : "-- MB/s"}</span>
                        </div>
                    </div>
                    <button class="node-speed-test-btn" title="Speed Test">
                        <i class="fa-solid fa-gauge-high"></i>
                    </button>
                </div>
                <div class="node-details">
                    <span class="cert-tag">${node.cert}</span>
                    <span class="ip-tag">${node.ip}</span>
                </div>
            `;

            // Individual refresh button
            const refreshBtn = card.querySelector(".node-refresh-btn");
            refreshBtn.addEventListener("click", (e) => {
                e.stopPropagation();
                testNodeLatency(node);
            });

            // Speed test button
            const speedTestBtn = card.querySelector(".node-speed-test-btn");
            speedTestBtn.addEventListener("click", (e) => {
                e.stopPropagation();
                testNodeSpeed(node, speedTestBtn);
            });

            // On click: Select node and go to Dashboard
            card.addEventListener("click", () => {
                selectNode(node);
                document.querySelector('[data-target="dashboard"]').click();
            });

            nodeGrid.appendChild(card);
        });
    }

    /* --- DASHBOARD LOGIC --- */
    const toggleBtn = document.getElementById("toggle-btn");

    // Toggle Connection Logic
    if (toggleBtn) {
        toggleBtn.addEventListener("click", () => {
            currentState.isConnected = !currentState.isConnected;
            updateConnectionState();
        });
    }

    function updateConnectionState() {
        if (!toggleBtn) return;

        const indicator = document.querySelector(".status-indicator");
        const uptimeEl = document.getElementById("uptime");
        const statusBadge = document.querySelector(".status-badge");

        if (currentState.isConnected) {
            toggleBtn.textContent = "Stop";
            toggleBtn.className = "btn-toggle stop";
            currentState.startTime = new Date();

            if (indicator) {
                indicator.classList.add("connected");
                indicator.querySelector(".text").textContent = "Connected";
            }
            if (statusBadge) {
                statusBadge.textContent = "Active";
                statusBadge.classList.add("connected");
            }
        } else {
            toggleBtn.textContent = "Start";
            toggleBtn.className = "btn-toggle start";
            currentState.startTime = null;
            if (uptimeEl) uptimeEl.textContent = "00:00:00";

            if (indicator) {
                indicator.classList.remove("connected");
                indicator.querySelector(".text").textContent = "Disconnected";
            }
            if (statusBadge) {
                statusBadge.textContent = "Idle";
                statusBadge.classList.remove("connected");
            }
        }
    }

    function selectNode(node) {
        currentState.selectedNodeId = node.id;
        updateDashboard(node);
        renderNodes();
    }

    function updateDashboard(node) {
        const nameEl = document.getElementById("dashboard-node-name");
        const ipEl = document.getElementById("dashboard-node-ip");
        const flagImg = document.getElementById("dashboard-flag");
        const maxSpeedEl = document.getElementById("dashboard-max-speed");

        if (nameEl) nameEl.textContent = node.name;
        if (ipEl) ipEl.textContent = node.ip;
        if (flagImg) flagImg.src = getFlagUrl(node.country_code);

        // Mock Max Speed
        const maxSpeed = Math.floor(Math.random() * 50 + 50); // 50-100 MB/s
        if (maxSpeedEl) maxSpeedEl.textContent = `Max: ${maxSpeed} MB/s`;
    }

    // Uptime Timer
    setInterval(() => {
        if (!currentState.startTime) return;
        const now = new Date();
        const diff = Math.floor((now - currentState.startTime) / 1000);
        const hours = String(Math.floor(diff / 3600)).padStart(2, "0");
        const minutes = String(Math.floor((diff % 3600) / 60)).padStart(2, "0");
        const seconds = String(diff % 60).padStart(2, "0");
        const uptimeEl = document.getElementById("uptime");
        if (uptimeEl) uptimeEl.textContent = `${hours}:${minutes}:${seconds}`;
    }, 1000);

    // Traffic Simulation
    setInterval(() => {
        const up = (Math.random() * 2).toFixed(1);
        const down = (Math.random() * 10).toFixed(1);
        const upEl = document.getElementById("upload-speed");
        const downEl = document.getElementById("download-speed");
        if (upEl) upEl.textContent = `${up} MB/s`;
        if (downEl) downEl.textContent = `${down} MB/s`;
    }, 1000);

    // Test Simulation Logic
    const dashTestAllBtn = document.getElementById("dash-test-all-btn");
    const dashTestLatBtn = document.getElementById("dash-test-lat-btn");
    const dashTestDnsBtn = document.getElementById("dash-test-dns-btn");

    if (dashTestAllBtn) {
        dashTestAllBtn.addEventListener("click", () => {
            runLatencyTest(dashTestAllBtn, true);
            runDnsTest(dashTestAllBtn, true);
            runServiceTest(dashTestAllBtn, true);
        });
    }

    if (dashTestLatBtn) {
        dashTestLatBtn.addEventListener("click", () => {
            runLatencyTest(dashTestLatBtn);
        });
    }

    if (dashTestDnsBtn) {
        dashTestDnsBtn.addEventListener("click", () => {
            runDnsTest(dashTestDnsBtn);
        });
    }

    const dashTestServiceBtn = document.getElementById("dash-test-service-btn");
    if (dashTestServiceBtn) {
        dashTestServiceBtn.addEventListener("click", () => {
            runServiceTest(dashTestServiceBtn);
        });
    }

    // Update Test All to include Service Test
    if (dashTestAllBtn) {
        // Remove old listener to avoid duplicates if re-running (but here we are editing the file directly)
        // actually we are replacing the block.
        // We need to redefine the listener entirely or just append.
        // The previous listener was:
        /*
        dashTestAllBtn.addEventListener("click", () => {
            runLatencyTest(dashTestAllBtn, true);
            runDnsTest(dashTestAllBtn, true);
        });
        */
        // We will replace the whole block in a moment.
    }

    function runLatencyTest(btn, isAll = false) {
        if (btn) btn.disabled = true;
        const pingEl = document.getElementById("ping-result");
        const packetLossEl = document.getElementById("packet-loss");
        const googleEl = document.getElementById("google-ping");

        if (pingEl) pingEl.textContent = "-- ms";
        if (packetLossEl) packetLossEl.textContent = "-- %";
        if (googleEl) googleEl.textContent = "-- ms";

        setTimeout(() => {
            const ping = Math.floor(Math.random() * 50) + 10;
            const loss = (Math.random() * 1).toFixed(1);
            const google = Math.floor(Math.random() * 60) + 20;

            if (pingEl) pingEl.textContent = `${ping} ms`;
            if (packetLossEl) {
                packetLossEl.textContent = `${loss} %`;
                packetLossEl.style.color =
                    loss < 0.5
                        ? "var(--success-color)"
                        : "var(--warning-color)";
            }
            if (googleEl) googleEl.textContent = `${google} ms`;

            if (btn && !isAll) btn.disabled = false;
            // If isAll, we let the longest test re-enable or just simple timeout
            if (isAll && dashTestAllBtn) dashTestAllBtn.disabled = false;
        }, 1500);
    }

    function runDnsTest(btn, isAll = false) {
        if (btn && !isAll) btn.disabled = true;
        const dnsEl = document.getElementById("dns-leak");
        const pollutionEl = document.getElementById("dns-pollution");

        if (dnsEl) dnsEl.textContent = "--";
        if (pollutionEl) pollutionEl.textContent = "--";

        setTimeout(() => {
            const dnsStatus =
                Math.random() > 0.1 ? "Safe (No Leak)" : "Leaking";
            const pollutionStatus = Math.random() > 0.1 ? "Clean" : "Polluted";

            if (dnsEl) {
                dnsEl.textContent = dnsStatus;
                dnsEl.style.color = dnsStatus.includes("Safe")
                    ? "var(--success-color)"
                    : "var(--danger-color)";
            }

            if (pollutionEl) {
                pollutionEl.textContent = pollutionStatus;
                pollutionEl.style.color =
                    pollutionStatus === "Clean"
                        ? "var(--success-color)"
                        : "var(--danger-color)";
            }

            if (btn && !isAll) btn.disabled = false;
        }, 2000);
    }

    function runServiceTest(btn, isAll = false) {
        if (btn && !isAll) btn.disabled = true;

        const openai = document.getElementById("service-openai");
        const netflix = document.getElementById("service-netflix");
        const disney = document.getElementById("service-disney");

        [openai, netflix, disney].forEach((el) => {
            if (el) {
                el.textContent = "--";
                el.style.color = "var(--text-secondary)";
            }
        });

        setTimeout(() => {
            const check = (el) => {
                if (!el) return;
                const unlocked = Math.random() > 0.3; // 70% success
                el.textContent = unlocked ? "Unlocked" : "Failed";
                el.style.color = unlocked
                    ? "var(--success-color)"
                    : "var(--danger-color)";
            };

            check(openai);
            check(netflix);
            check(disney);

            if (btn && !isAll) btn.disabled = false;
            // Re-enable Test All if it was the caller
            if (isAll && dashTestAllBtn) dashTestAllBtn.disabled = false;
        }, 2500);
    }

    // Mode Selector Logic
    const modeBtns = document.querySelectorAll(".mode-btn");
    modeBtns.forEach((btn) => {
        btn.addEventListener("click", () => {
            modeBtns.forEach((b) => b.classList.remove("active"));
            btn.classList.add("active");
            // Here you would trigger actual mode switch logic
            console.log("Switched to mode:", btn.dataset.mode);
        });
    });

    /* --- SETTINGS PAGE LOGIC --- */
    const saveSettingsBtn = document.getElementById("save-settings-btn");

    if (saveSettingsBtn) {
        saveSettingsBtn.addEventListener("click", () => {
            // Gather all values (normally you'd save to a config object)
            // Gather all values into a structured config object (Sing-Box Schema)
            const config = {
                log: {
                    level: document.getElementById("log-level")?.value,
                    timestamp:
                        document.getElementById("log-timestamp")?.checked,
                },
                dns: {
                    servers: [
                        {
                            tag: "remote",
                            address:
                                document.getElementById("dns-remote")?.value,
                        },
                        {
                            tag: "local",
                            address:
                                document.getElementById("dns-direct")?.value,
                        },
                    ],
                    strategy: document.getElementById("dns-strategy")?.value,
                    fakeip: {
                        enabled: document.getElementById("dns-fakeip")?.checked,
                    },
                },
                inbounds: [
                    {
                        type: "mixed",
                        listen_port: parseInt(
                            document.getElementById("inbound-mixed-port")
                                ?.value,
                        ),
                        allow_lan:
                            document.getElementById("inbound-allow-lan")
                                ?.checked,
                    },
                    {
                        type: "tun",
                        enabled:
                            document.getElementById("tun-enabled")?.checked,
                        stack: document.getElementById("tun-stack")?.value,
                        mtu: parseInt(
                            document.getElementById("tun-mtu")?.value,
                        ),
                        auto_route:
                            document.getElementById("tun-auto-route")?.checked,
                        strict_route:
                            document.getElementById("tun-strict-route")
                                ?.checked,
                    },
                ],
                route: {
                    final: document.getElementById("route-final")?.value,
                    auto_detect_interface:
                        document.getElementById("route-auto-detect")?.checked,
                },
                ntp: {
                    enabled: document.getElementById("ntp-enabled")?.checked,
                    server: document.getElementById("ntp-server")?.value,
                },
                experimental: {
                    clash_api: {
                        external_controller:
                            document.getElementById("exp-clash-api")?.value,
                        external_ui: "dashboard",
                    },
                    cache_file: {
                        enabled:
                            document.getElementById("exp-cache-file")?.checked,
                    },
                },
            };

            console.log("Saving Configuration:", config);

            // Animation
            const originalText = saveSettingsBtn.innerHTML;
            saveSettingsBtn.disabled = true;
            saveSettingsBtn.innerHTML =
                '<i class="fa-solid fa-spinner fa-spin"></i> Saving...';

            setTimeout(() => {
                saveSettingsBtn.disabled = false;
                saveSettingsBtn.innerHTML = originalText;
                alert(
                    "Configuration saved successfully! Core restart may be required.",
                );
            }, 1000);
        });
    }

    /* --- SUBSCRIPTION LOGIC --- */
    const subList = document.getElementById("sub-list");
    const updateSubsBtn = document.getElementById("update-subs-btn");
    const importBtn = document.getElementById("import-sub-btn");
    
    // Modal Elements
    const importModal = document.getElementById("import-modal");
    const closeModalBtn = document.getElementById("close-modal-btn");
    const cancelModalBtn = document.getElementById("cancel-modal-btn");
    const saveModalBtn = document.getElementById("save-modal-btn");
    
    // Form Inputs
    const nameInput = document.getElementById("modal-sub-name");
    const urlInput = document.getElementById("modal-sub-url");
    const autoInput = document.getElementById("modal-sub-auto");
    const intervalSelect = document.getElementById("modal-sub-interval");
    const uaInput = document.getElementById("modal-sub-ua");

    let subscriptions = [
        {
            id: 1,
            name: "Lucas Premium Sub",
            url: "https://example.com/sub/premium",
            updateInterval: "12h",
            autoUpdate: true,
            userAgent: "v2rayN/1.0",
            lastUpdated: "10 mins ago",
            nodesCount: 42,
            upload: "12.5 GB",
            download: "105.2 GB",
            totalTraffic: "500 GB",
            expireTime: "2026-12-31",
            resetTime: "2026-05-15"
        },
        {
            id: 2,
            name: "Free Backup Nodes",
            url: "https://example.com/sub/free",
            updateInterval: "24h",
            autoUpdate: true,
            userAgent: "",
            lastUpdated: "5 hours ago",
            nodesCount: 15,
            upload: "1.2 GB",
            download: "5.4 GB",
            totalTraffic: "50 GB",
            expireTime: "2026-08-01",
            resetTime: "2026-05-01"
        }
    ];

    function renderSubscriptions() {
        if (!subList) return;
        subList.innerHTML = "";

        if (subscriptions.length === 0) {
            subList.innerHTML = `<div class="empty-state" id="sub-empty-state" style="grid-column: 1 / -1;">No subscriptions added.</div>`;
            return;
        }

        subscriptions.forEach((sub) => {
            const card = document.createElement("div");
            card.className = "sub-card";
            card.innerHTML = `
                <div class="sub-card-header">
                    <div style="flex: 1; overflow: hidden;">
                        <h3 class="sub-card-title">${sub.name}</h3>
                        <div class="sub-card-url">${sub.url}</div>
                    </div>
                </div>
                <div class="sub-card-meta">
                    <div class="sub-meta-item" title="Nodes Count">
                        <i class="fa-solid fa-server"></i> ${sub.nodesCount || 0}
                    </div>
                    <div class="sub-meta-item" title="Update Interval">
                        <i class="fa-solid fa-clock-rotate-left"></i> ${sub.updateInterval}
                    </div>
                    <div class="sub-meta-item" title="Last Updated">
                        <i class="fa-solid fa-check-double"></i> ${sub.lastUpdated || 'Never'}
                    </div>
                </div>
                <div class="sub-traffic-grid">
                    <div class="traffic-stat">
                        <span class="traffic-label">Upload</span>
                        <span class="traffic-val up"><i class="fa-solid fa-arrow-up"></i> ${sub.upload || '0 B'}</span>
                    </div>
                    <div class="traffic-stat">
                        <span class="traffic-label">Download</span>
                        <span class="traffic-val down"><i class="fa-solid fa-arrow-down"></i> ${sub.download || '0 B'}</span>
                    </div>
                    <div class="traffic-stat full">
                        <span class="traffic-label">Total Traffic</span>
                        <span class="traffic-val">${sub.totalTraffic || '0 B'}</span>
                    </div>
                </div>
                <div class="sub-dates">
                    <div>
                        <span>Expire Time:</span>
                        <strong>${sub.expireTime || 'N/A'}</strong>
                    </div>
                    <div>
                        <span>Reset Time:</span>
                        <strong>${sub.resetTime || 'N/A'}</strong>
                    </div>
                </div>
                <div class="sub-card-actions">
                    <button class="btn-icon update-single-sub" data-id="${sub.id}" title="Update Now">
                        <i class="fa-solid fa-rotate"></i>
                    </button>
                    <button class="btn-icon delete-sub" data-id="${sub.id}" title="Delete">
                        <i class="fa-solid fa-trash"></i>
                    </button>
                </div>
            `;
            subList.appendChild(card);
        });

        // Delete Handler
        document.querySelectorAll(".delete-sub").forEach((btn) => {
            btn.addEventListener("click", (e) => {
                const id = parseInt(e.currentTarget.getAttribute("data-id"));
                if (confirm("Are you sure you want to delete this subscription?")) {
                    subscriptions = subscriptions.filter((s) => s.id !== id);
                    renderSubscriptions();
                }
            });
        });

        // Update Single Sub Handler
        document.querySelectorAll(".update-single-sub").forEach((btn) => {
            btn.addEventListener("click", (e) => {
                const icon = e.currentTarget.querySelector('i');
                icon.classList.add('fa-spin');
                setTimeout(() => {
                    icon.classList.remove('fa-spin');
                    const subId = parseInt(e.currentTarget.getAttribute("data-id"));
                    const sub = subscriptions.find(s => s.id === subId);
                    if (sub) {
                        sub.lastUpdated = "Just now";
                        sub.nodesCount = Math.floor(Math.random() * 50) + 10;
                        renderSubscriptions();
                    }
                }, 1000);
            });
        });
    }

    // Modal Logic
    function openModal() {
        if (!importModal) return;
        nameInput.value = "";
        urlInput.value = "";
        autoInput.checked = true;
        intervalSelect.value = "12h";
        uaInput.value = "";
        importModal.classList.add("active");
    }

    function closeModal() {
        if (!importModal) return;
        importModal.classList.remove("active");
    }

    if (importBtn) importBtn.addEventListener("click", openModal);
    if (closeModalBtn) closeModalBtn.addEventListener("click", closeModal);
    if (cancelModalBtn) cancelModalBtn.addEventListener("click", closeModal);
    
    // Close on overlay click
    if (importModal) {
        importModal.addEventListener("click", (e) => {
            if (e.target === importModal) closeModal();
        });
    }

    // Save Subscription
    if (saveModalBtn) {
        saveModalBtn.addEventListener("click", () => {
            const url = urlInput.value.trim();
            const name = nameInput.value.trim() || "New Subscription";
            
            if (!url) {
                alert("URL is required!");
                return;
            }

            const newSub = {
                id: Date.now(),
                name: name,
                url: url,
                updateInterval: intervalSelect.value,
                autoUpdate: autoInput.checked,
                userAgent: uaInput.value.trim(),
                lastUpdated: "Just now",
                nodesCount: Math.floor(Math.random() * 50) + 10,
                upload: "0 GB",
                download: "0 GB",
                totalTraffic: "100 GB",
                expireTime: "2027-01-01",
                resetTime: "Next month"
            };
            
            subscriptions.push(newSub);
            closeModal();
            renderSubscriptions();
        });
    }

    if (updateSubsBtn) {
        updateSubsBtn.addEventListener("click", () => {
            const originalText = updateSubsBtn.innerHTML;
            updateSubsBtn.disabled = true;
            updateSubsBtn.innerHTML = '<i class="fa-solid fa-spinner fa-spin"></i> Updating...';
            setTimeout(() => {
                updateSubsBtn.disabled = false;
                updateSubsBtn.innerHTML = originalText;
                subscriptions.forEach(sub => sub.lastUpdated = "Just now");
                renderSubscriptions();
                alert("All subscriptions updated!");
            }, 1500);
        });
    }

    renderSubscriptions();

    // Init
    if (typeof mockNodes !== "undefined" && mockNodes.length > 0) {
        const initialNode =
            mockNodes.find((n) => n.id === currentState.selectedNodeId) ||
            mockNodes[0];
        updateConnectionState();
        updateDashboard(initialNode);
        testAllNodes(); // Auto-test all nodes on load
    }
});
