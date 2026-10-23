// Every step the WebDriver walk checks, by section. walk.mjs runs them in the
// plan's order.
import data from "./data.mjs";
import dialog from "./dialog.mjs";
import dropdowns from "./dropdowns.mjs";
import filters from "./filters.mjs";
import history from "./history.mjs";
import loading from "./loading.mjs";
import notices from "./notices.mjs";
import places from "./places.mjs";
import rest from "./rest.mjs";
import scrollbars from "./scrollbars.mjs";
import tooltips from "./tooltips.mjs";

export default [data, loading, places, dropdowns, tooltips, scrollbars, notices, filters, dialog, history, rest];
