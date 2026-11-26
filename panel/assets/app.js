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
    const tog = ev.target.closest("[data-toggle=side]");
    if (tog) document.body.classList.toggle("side-open");
  });

  // tempo real (Server-Sent Events)
  const page = document.body.getAttribute("data-live");
  const pill = $("#live-pill");

  function setLive(state) {
    if (!pill) return;
    pill.classList.toggle("on", state);
    pill.classList.toggle("off", !state);
    $(".txt", pill).textContent = state ? "Ao vivo" : "Reconectando";
  }

  if (page && window.EventSource) {
    const es = new EventSource("/live?page=" + encodeURIComponent(page));
    es.addEventListener("open", () => setLive(true));
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
      setLive(false);
      if (!probe) {
        check();
        probe = setInterval(check, 4000);
      }
    });
    es.addEventListener("logout", () => {
      window.location.href = "/login";
    });
    es.addEventListener("patch", (ev) => {
      setLive(true);
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
  } else {
    setLive(false);
  }
})();
