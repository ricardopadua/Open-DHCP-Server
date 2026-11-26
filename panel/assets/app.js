// Painel Open DHCP. Sem dependências; carregado com CSP script-src 'self'.
(function () {
  const $ = (s, el) => (el || document).querySelector(s);
  const $$ = (s, el) => Array.from((el || document).querySelectorAll(s));

  // confirmação e envio único
  document.addEventListener("submit", (ev) => {
    const form = ev.target;
    const msg = form.getAttribute("data-confirm");
    if (msg && !window.confirm(msg)) {
      ev.preventDefault();
      return;
    }
    // fechar notificação: sem recarregar a página (o fluxo ao vivo atualiza o resto)
    if (form.hasAttribute("data-ajax")) {
      ev.preventDefault();
      const item = form.closest(".alert, .note");
      if (item) item.hidden = true;
      const undo = () => {
        if (item) item.hidden = false;
      };
      fetch(form.action, {
        method: "POST",
        headers: { "x-requested-with": "fetch" },
        body: new URLSearchParams(new FormData(form)),
      })
        .then((r) => {
          if (!r.ok) undo();
        })
        .catch(undo);
      return;
    }
    $$("button[type=submit]", form).forEach((b) => {
      b.disabled = true;
      b.classList.add("busy");
    });
  });

  // filtros: texto (data-filter) + tipo (chips com data-kind)
  function applyFilters() {
    const tables = new Set();
    $$("[data-filter]").forEach((i) => tables.add(i.getAttribute("data-filter")));
    $$("[data-chips]").forEach((c) => tables.add(c.getAttribute("data-chips")));
    tables.forEach((sel) => {
      const table = $(sel);
      if (!table) return;
      const input = $(`[data-filter="${sel}"]`);
      const q = input ? input.value.trim().toLowerCase() : "";
      const chip = $(`[data-chips="${sel}"] .chip.active`);
      const kind = chip ? chip.getAttribute("data-kind") : "";
      $$("tbody tr", table).forEach((tr) => {
        const okText = q === "" || tr.textContent.toLowerCase().includes(q);
        const okKind = !kind || tr.getAttribute("data-kind") === kind;
        tr.hidden = !(okText && okKind);
      });
    });
  }

  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") $$("details.notif[open], details.menu[open]").forEach((d) => (d.open = false));
  });

  document.addEventListener("input", (ev) => {
    if (ev.target.hasAttribute && ev.target.hasAttribute("data-filter")) applyFilters();
  });

  document.addEventListener("click", (ev) => {
    const chip = ev.target.closest(".chip[data-kind]");
    if (chip) {
      $$(".chip", chip.parentElement).forEach((c) => c.classList.toggle("active", c === chip));
      applyFilters();
    }
    const tt = ev.target.closest("[data-theme-toggle]");
    if (tt) {
      const dark = !document.documentElement.classList.contains("dark");
      document.documentElement.classList.toggle("dark", dark);
      localStorage.setItem("odp-theme", dark ? "dark" : "light");
    }
    const close = ev.target.closest("[data-close]");
    if (close) close.closest(".alert").hidden = true;
    // fecha as listas suspensas (notificações, idioma) ao clicar fora delas
    $$("details.notif[open], details.menu[open]").forEach((d) => {
      if (!d.contains(ev.target)) d.open = false;
    });
    const tog = ev.target.closest("[data-toggle=side]");
    if (tog) document.body.classList.toggle("side-open");
  });

  // ---------- formulários de rede: máscara/prefixo, quantidade de IPs, tempo de lease ----------
  // (o servidor valida tudo de novo; aqui é só ajuda enquanto a pessoa digita)
  const ip2n = (s) => {
    const m = /^\s*(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\s*$/.exec(s || "");
    if (!m) return null;
    const p = m.slice(1).map(Number);
    if (p.some((x) => x > 255)) return null;
    return p[0] * 16777216 + p[1] * 65536 + p[2] * 256 + p[3];
  };
  const n2ip = (n) => [24, 16, 8, 0].map((s) => (n >>> s) & 255).join(".");
  const maskOf = (p) => (p === 0 ? 0 : (0xffffffff << (32 - p)) >>> 0);
  const prefixOf = (m) => {
    if (m === null) return null;
    for (let p = 0; p <= 32; p++) if (maskOf(p) === m) return p;
    return null;
  };
  const hostsOf = (p) => (p === 32 ? 1 : p === 31 ? 2 : 2 ** (32 - p) - 2);
  const bounds = (ip, p) => {
    const m = maskOf(p);
    const net = (ip & m) >>> 0;
    return [net, (net | (~m >>> 0)) >>> 0];
  };
  const parseRange = (line) => {
    const parts = line.replace(/\s/g, "").split("-");
    if (parts.length !== 2) return null;
    const a = ip2n(parts[0]);
    const b = ip2n(parts[1]);
    return a === null || b === null || a > b ? null : [a, b];
  };
  // mesmo formato do i18n do servidor: "{}" na ordem ou "{0}", "{1}"...
  const fill = (tpl, ...args) => {
    let i = 0;
    return (tpl || "").replace(/\{(\d*)\}/g, (_, d) => String(args[d === "" ? i++ : Number(d)] ?? ""));
  };
  const netInfo = $("#net-info");

  function updateNet() {
    if (!netInfo) return;
    const d = netInfo.dataset;
    const maskIn = $("[data-mask]");
    const prefix = maskIn ? prefixOf(ip2n(maskIn.value)) : null;
    const lines = ($("[data-ranges]") || { value: "" }).value.split("\n").map((l) => l.trim()).filter(Boolean);
    const out = [];
    const ranges = [];
    let total = 0;
    for (const l of lines) {
      const r = parseRange(l);
      if (!r) {
        out.push([fill(d.tInvalid, l), "bad"]);
        continue;
      }
      ranges.push(r);
      total += r[1] - r[0] + 1;
      out.push([fill(d.tCount, n2ip(r[0]) + " – " + n2ip(r[1]), r[1] - r[0] + 1), ""]);
    }
    if (ranges.length > 1) out.push([fill(d.tTotal, total), ""]);
    if (prefix !== null && ranges.length) {
      const [net, bc] = bounds(ranges[0][0], prefix);
      out.unshift([fill(d.tNet, n2ip(net), prefix, n2ip(bc), hostsOf(prefix)), "net"]);
      for (const [a, b] of ranges) {
        const label = n2ip(a) + "-" + n2ip(b);
        const [na, ba] = bounds(a, prefix);
        if (bounds(b, prefix)[0] !== na) out.push([fill(d.tOutside, label, n2ip(na), prefix), "bad"]);
        else if (prefix <= 30 && (a === na || b === ba)) out.push([fill(d.tEdge, label), "bad"]);
      }
      const gws = ($("[data-router]") || { value: "" }).value.split(/[,;\s]+/).map(ip2n).filter((x) => x !== null);
      for (const g of gws) {
        if (bounds(g, prefix)[0] !== net) {
          out.push([fill(d.tGwOut, n2ip(g), n2ip(net), prefix), "bad"]);
        } else {
          const r = ranges.find(([a, b]) => g >= a && g <= b);
          if (r) out.push([fill(d.tGwIn, n2ip(g), n2ip(r[0]) + "-" + n2ip(r[1])), "bad"]);
        }
      }
    }
    netInfo.replaceChildren(
      ...out.map(([txt, cls]) => {
        const s = document.createElement("span");
        s.textContent = txt;
        if (cls) s.className = cls;
        return s;
      })
    );
  }

  function helperRange() {
    const a = ip2n(($("#rh-start") || {}).value);
    const c = parseInt(($("#rh-count") || {}).value, 10);
    if (a === null || !(c >= 1)) return null;
    const b = a + c - 1;
    return b > 0xffffffff ? null : [a, b];
  }

  function updateHelper() {
    const el = $("#rh-end");
    if (!el || !netInfo) return;
    const r = helperRange();
    el.textContent = r ? fill(netInfo.dataset.tUntil, n2ip(r[1])) : "";
  }

  function syncPrefix(maskIn) {
    const sel = maskIn.closest("label").querySelector("[data-prefix]");
    const p = prefixOf(ip2n(maskIn.value));
    if (sel) sel.value = p === null ? "" : String(p);
  }

  document.addEventListener("input", (ev) => {
    const el = ev.target;
    if (!el.matches) return;
    if (el.matches("[data-mask]")) syncPrefix(el);
    if (el.matches("[data-mask], [data-ranges], [data-router]")) updateNet();
    if (el.matches("#rh-start, #rh-count")) updateHelper();
  });

  document.addEventListener("change", (ev) => {
    const el = ev.target;
    if (el.matches && el.matches("[data-prefix]") && el.value !== "") {
      const m = el.closest("label").querySelector("[data-mask]");
      if (m) m.value = n2ip(maskOf(Number(el.value)));
      updateNet();
    }
  });

  document.addEventListener("click", (ev) => {
    // assistente: primeiro IP + quantidade → linha INÍCIO-FIM (e a máscara, se estiver vazia)
    if (ev.target.closest("[data-add-range]")) {
      const r = helperRange();
      if (!r) {
        $("#rh-end").textContent = netInfo.dataset.tBadStart;
        return;
      }
      const ta = $("[data-ranges]");
      const line = n2ip(r[0]) + "-" + n2ip(r[1]);
      ta.value = (ta.value.trim() ? ta.value.replace(/\s*$/, "\n") : "") + line;
      const mask = $("[data-mask]");
      if (mask && !mask.value.trim()) {
        // a menor rede em que a faixa cabe sem usar o endereço de rede nem o broadcast
        for (let p = 30; p >= 8; p--) {
          const [n, bc] = bounds(r[0], p);
          if (bounds(r[1], p)[0] === n && r[0] !== n && r[1] !== bc) {
            mask.value = n2ip(maskOf(p));
            syncPrefix(mask);
            break;
          }
        }
      }
      $("#rh-start").value = "";
      $("#rh-count").value = "";
      updateHelper();
      updateNet();
    }
    // atalhos do tempo de lease
    const ls = ev.target.closest("[data-lease-set]");
    if (ls) {
      const box = ls.closest(".lease-field");
      const secs = Number(ls.getAttribute("data-lease-set"));
      const v = $("[data-lease-value]", box);
      const u = $("[data-lease-unit]", box);
      if (secs === 0) {
        v.value = "0";
      } else {
        const [unit, mult] = [["d", 86400], ["h", 3600], ["min", 60], ["s", 1]].find(([, m]) => secs % m === 0);
        v.value = String(secs / mult);
        u.value = unit;
      }
    }
  });

  updateNet();

  // tempo real (Server-Sent Events)
  const page = document.body.getAttribute("data-live");
  if (page && window.EventSource) {
    const es = new EventSource("/live?page=" + encodeURIComponent(page));
    // Desconectado: a cada 4 s confere se o painel voltou. Sem sessão (painel reiniciado ou
    // sessão expirada) vai para o login; com sessão e o fluxo fechado, recarrega a página.
    let probe = null;
    const check = () =>
      fetch("/live?page=check", { redirect: "manual" })
        .then((r) => {
          if (r.body) r.body.cancel();
          if (r.type === "opaqueredirect" || r.status === 303) {
            window.location.href = "/login";
          } else if (r.ok && es.readyState === EventSource.CLOSED) {
            window.location.reload();
          } else if (r.ok) {
            clearInterval(probe);
            probe = null;
          }
        })
        .catch(() => {});
    es.addEventListener("error", () => {
      if (!probe) {
        check();
        probe = setInterval(check, 4000);
      }
    });
    es.addEventListener("logout", () => {
      window.location.href = "/login";
    });
    es.addEventListener("patch", (ev) => {
      let data;
      try {
        data = JSON.parse(ev.data);
      } catch (_) {
        return;
      }
      const busy = document.querySelector("button.busy");
      for (const [sel, html] of Object.entries(data)) {
        const el = $(sel);
        // não troca o bloco em que a pessoa acabou de enviar um formulário
        if (el && !(busy && el.contains(busy))) el.innerHTML = html;
      }
      applyFilters();
    });
    window.addEventListener("pagehide", () => es.close());
  }
})();
