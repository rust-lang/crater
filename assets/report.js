function setup_buttons() {
    let buttons = document.querySelectorAll(".toggle");
    for (let i = 0; i < buttons.length; i++) {
        buttons[i].addEventListener("click", function(e) {
            e.preventDefault();

            this.classList.toggle("selected");

            let selector = this.getAttribute("data-toggle");
            let elements = document.querySelectorAll(selector);
            for (let i = 0; i < elements.length; i++) {
                elements[i].classList.toggle("hidden");
            }
        }.bind(buttons[i]));
    }
}

setup_buttons();
