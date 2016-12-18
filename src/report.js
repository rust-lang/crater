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
    let unknownEl = document.querySelector("#c-unknown .count");
    let sameBuildFailEl = document.querySelector("#c-same-build-fail .count");
    let sameTestFailEl = document.querySelector("#c-same-test-fail .count");
    let sameTestPassEl = document.querySelector("#c-same-test-pass .count");

    regressedEl.innerHTML = summary.regressed;
    fixedEl.innerHTML = summary.fixed;
    unknownEl.innerHTML = summary.unknown;
    sameBuildFailEl.innerHTML = summary.sameBuildFail;
    sameTestFailEl.innerHTML = summary.sameTestFail;
    sameTestPassEl.innerHTML = summary.sameTestPass;

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
    let unknown = 0;
    let sameBuildFail = 0;
    let sameTestFail = 0;
    let sameTestPass = 0;

    for (crate of results.crates) {
	if (crate.res == "Regressed") {
	    regressed += 1;
	} else if (crate.res == "Fixed") {
	    fixed += 1;
	} else if (crate.res == "Unknown") {
	    unknown += 1;
	} else if (crate.res == "SameBuildFail") {
	    sameBuildFail += 1;
	} else if (crate.res == "SameTestFail") {
	    sameTestFail += 1;
	} else if (crate.res == "SameTestPass") {
	    sameTestPass += 1;
	} else {
	    throw "unknown test status";
	}
    }

    return {
	regressed: regressed,
	fixed: fixed,
	unknown: unknown,
	sameBuildFail: sameBuildFail,
	sameTestFail: sameTestFail,
	sameTestPass: sameTestPass
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
    } else if (res == "Unknown") {
	return "unknown";
    } else if (res == "SameBuildFail") {
	return "same-build-fail";
    } else if (res == "SameTestFail") {
	return "same-test-fail";
    } else if (res == "SameTestPass") {
	return "same-test-pass";
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
    if (res == "BuildFail") {
	return "build-fail";
    } else if (res == "TestFail") {
	return "test-fail";
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
