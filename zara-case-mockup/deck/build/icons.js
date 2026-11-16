const React = require("react"), RDS = require("react-dom/server"), sharp = require("sharp"), fa = require("react-icons/fa");
const names = ["FaEyeSlash", "FaQuestionCircle", "FaHourglassHalf", "FaMobileAlt", "FaCamera", "FaCheckCircle", "FaGift", "FaLayerGroup", "FaSearchDollar", "FaTags", "FaBolt", "FaWarehouse", "FaStore", "FaShoppingBag", "FaUserFriends", "FaChartLine", "FaClock", "FaShieldAlt", "FaRedoAlt", "FaUsers"];
(async () => {
  for (const n of names) for (const [tag, col] of [["w", "#FFFFFF"], ["k", "#111111"]]) {
    const svg = RDS.renderToStaticMarkup(React.createElement(fa[n], { color: col, size: 256 }));
    await sharp(Buffer.from(svg)).resize(256, 256).png().toFile(`img/ic/${n}-${tag}.png`);
  }
  console.log("icons", names.length);
})();
