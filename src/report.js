let config = null;
let results = null;

window.onload = function() {
    let configReq = new XMLHttpRequest();
    configReq.addEventListener("load", function() { loadConfig(configReq) });
    configReq.overrideMimeType("application/json");
    configReq.open("GET", "config.json");
    configReq.send();

    let resultsReq = new XMLHttpRequest();
    resultsReq.addEventListener("load", function() { loadResults(resultsReq) });
    resultsReq.overrideMimeType("application/json");
    resultsReq.open("GET", "results.json");
    resultsReq.send();

    setUpButtons();
};

function loadConfig(req) {
    config = JSON.parse(req.responseText);
    if (config != null && results != null) {
	begin(config, results);
    }
}

function loadResults(req) {
    results = JSON.parse(req.responseText);
    if (config != null && results != null) {
	begin(config, results);
    }
}

function begin(config, results) {
    let nameEl = document.getElementById("ex-name");

    nameEl.innerHTML = config.name;

    let tc1 = parseToolchain(config.toolchains[0]);
    let tc2 = parseToolchain(config.toolchains[1]);

    let tc1el = document.getElementById("ex-tc1");
    let tc2el = document.getElementById("ex-tc2");

    tc1el.innerHTML = tc1;
    tc2el.innerHTML = tc2;

    let cratesEl = document.getElementById("ex-crates");
    cratesEl.innerHTML = config.crates.length;

    let summary = calcSummary(results);

    let regressedEl = document.querySelector("#c-regressed .count");
    let fixedEl = document.querySelector("#c-fixed .count");
    let sameFailEl = document.querySelector("#c-same-fail .count");
    let sameBuildPassEl = document.querySelector("#c-same-build-pass .count");
    let sameTestPassEl = document.querySelector("#c-same-test-pass .count");
    let unknownEl = document.querySelector("#c-unknown .count");

    regressedEl.innerHTML = summary.regressed;
    fixedEl.innerHTML = summary.fixed;
    sameFailEl.innerHTML = summary.sameFail;
    sameBuildPassEl.innerHTML = summary.sameBuildPass;
    sameTestPassEl.innerHTML = summary.sameTestPass;
    unknownEl.innerHTML = summary.unknown;

    // Creating the document will take a second. Lay out the summary first.
    let results_ = results;
    window.setTimeout(function() {
        insertResults(results_);
    }, 1);

    config = null;
    results = null;
}

function parseToolchain(tc) {
    if (tc["Dist"]) {
	return tc["Dist"];
    } else {
	throw "unsupported toolchain type";
    }
}

function calcSummary(results) {
    let regressed = 0;
    let fixed = 0;
    let sameFail = 0;
    let sameBuildPass = 0;
    let sameTestPass = 0;
    let unknown = 0;

    for (crate of results.crates) {
	if (crate.res == "Regressed") {
	    regressed += 1;
	} else if (crate.res == "Fixed") {
	    fixed += 1;
	} else if (crate.res == "SameFail") {
	    sameFail += 1;
	} else if (crate.res == "SameBuildPass") {
	    sameBuildPass += 1;
	} else if (crate.res == "SameTestPass") {
	    sameTestPass += 1;
	} else if (crate.res == "Unknown") {
	    unknown += 1;
	} else {
	    throw "unknown test status";
	}
    }

    return {
	regressed: regressed,
	fixed: fixed,
	sameFail: sameFail,
	sameBuildPass: sameBuildPass,
	sameTestPass: sameTestPass,
	unknown: unknown
    };
}

function insertResults(results) {
    let resultsTableEl = document.getElementById("results");

    for (crate of results.crates) {
	let name = crate.name;
	let res = jsonCrateResToCss(crate.res);
	let run1 = parseRunResult(crate.runs[0]);
	let run2 = parseRunResult(crate.runs[1]);

        function runToHtml(run) {
            if (run.log) {
	        return `<span><a href="${run.log}">${run.res}</a></span>`;
            } else {
	        return `<span>${run.res}</span>`;
            }
        }

	let html1 = runToHtml(run1);
	let html2 = runToHtml(run2);

	let row = `
	<div class="${res}">
	    <span>${name}</span>
	    ${html1}
	    ${html2}
        </div>
	`;

	let template = document.createElement("table");
	template.innerHTML = row;
	let newNode = template.childNodes[1];

	resultsTableEl.appendChild(newNode);
    }
}

function jsonCrateResToCss(res) {
    if (res == "Regressed") {
	return "regressed";
    } else if (res == "Fixed") {
	return "fixed";
    } else if (res == "SameFail") {
	return "same-fail";
    } else if (res == "SameBuildPass") {
	return "same-build-pass";
    } else if (res == "SameTestPass") {
	return "same-test-pass";
    } else if (res == "Unknown") {
	return "unknown";
    } else {
	throw "unknown test status";
    }
}

function parseRunResult(res) {
    if (res == null) {
	return {
	    res: "unknown",
	    log: null
	};
    } else {
	return {
	    res: jsonRunResToDisplay(res.res),
	    log: res.log
	};
    }
}

function jsonRunResToDisplay(res) {
    if (res == "Fail") {
	return "fail";
    } else if (res == "BuildPass") {
	return "build-pass";
    } else if (res == "TestPass") {
	return "test-pass";
    } else {
	throw "unknown test status";
    }
}

function setUpButtons() {
    let buttons = document.querySelectorAll("#controls > span");

    for (button_ of buttons) {
        let button = button_;
	button.addEventListener("click", function(event) {
	    let id = button.id;
	    let class_ = id.slice(2, id.length);
            let selector = `#results .${class_}`;

	    let rows = document.querySelectorAll(selector);
	    for (row of rows) {
		row.classList.toggle("visible");
	    }

	    button.classList.toggle("selected");
	});
    }
}
