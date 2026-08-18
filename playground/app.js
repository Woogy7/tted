const button = document.querySelector("#greet");

button.addEventListener("click", () => {
  const languages = ["Rust", "Ruby", "JavaScript", "Markdown"];
  const choice = languages[Math.floor(Math.random() * languages.length)];
  button.textContent = `Hello, ${choice}!`;
});
