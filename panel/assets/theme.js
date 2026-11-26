// Aplica o tema antes de desenhar a página (evita piscar). "auto" segue o sistema.
(function () {
  var t = localStorage.getItem("odp-theme") || "auto";
  var dark = t === "dark" || (t === "auto" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
})();
