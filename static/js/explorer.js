'use strict';

Chart.register(ChartZoom);

let chart = null;

const COLORS = [
  '#00b4d8', '#f4a261', '#ef233c', '#4cc9f0',
  '#7209b7', '#3a0ca3', '#4ade80', '#facc15',
  '#f87171', '#a78bfa', '#34d399', '#fb923c',
  '#60a5fa', '#e879f9',
];

function getPresetRange() {
  const preset = document.querySelector('input[name="range-preset"]:checked').value;
  const now = Math.floor(Date.now() / 1000);
  if (preset === 'custom') {
    const from = document.getElementById('range-from').value;
    const to = document.getElementById('range-to').value;
    return {
      from: from ? Math.floor(new Date(from).getTime() / 1000) : now - 48 * 3600,
      to: to ? Math.floor(new Date(to).getTime() / 1000) : now,
    };
  }
  const table = { '48h': 48*3600, '2w': 14*86400, '1m': 30*86400, '1y': 365*86400, '5y': 5*365*86400 };
  return { from: now - (table[preset] ?? 48*3600), to: now };
}

function getCF() {
  return document.querySelector('input[name="cf"]:checked').value;
}

function getSelected(id) {
  return Array.from(document.getElementById(id).selectedOptions).map(o => o.value);
}

async function fetchSensorHistory(sensorId, metrics, from, to, cf) {
  const url = `/api/sensors/${sensorId}/history?from=${from}&to=${to}&cf=${cf}&resolution=auto`;
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`HTTP ${resp.status} for ${sensorId}`);
  const data = await resp.json();

  const series = {};
  for (const metric of metrics) {
    series[metric] = { timestamps: [], values: [] };
  }

  for (const pt of data.datapoints) {
    for (const metric of metrics) {
      if (pt[metric] !== null && pt[metric] !== undefined) {
        series[metric].timestamps.push(pt.t * 1000); // ms for Chart.js
        series[metric].values.push(pt[metric]);
      }
    }
  }

  return series;
}

async function buildChart() {
  const sensors = getSelected('exp-sensor');
  const metrics = getSelected('exp-metrics');

  if (sensors.length === 0 || metrics.length === 0) {
    alert('Select at least one sensor and one metric.');
    return;
  }

  document.getElementById('chart-status').textContent = 'Loading data…';
  document.getElementById('chart-container').style.display = '';

  const { from, to } = getPresetRange();
  const cf = getCF();

  // Fetch all sensors in parallel
  let allData;
  try {
    allData = await Promise.all(sensors.map(s => fetchSensorHistory(s, metrics, from, to, cf)));
  } catch (e) {
    document.getElementById('chart-status').textContent = 'Error: ' + e.message;
    return;
  }

  // Build Chart.js datasets
  const datasets = [];
  let colorIdx = 0;
  for (let si = 0; si < sensors.length; si++) {
    const sensorId = sensors[si];
    const sensorLabel = document.querySelector(`#exp-sensor option[value="${sensorId}"]`).text;
    for (const metric of metrics) {
      const series = allData[si][metric];
      const color = COLORS[colorIdx++ % COLORS.length];
      datasets.push({
        label: `${sensorLabel} — ${metric}`,
        data: series.timestamps.map((t, i) => ({ x: t, y: series.values[i] })),
        borderColor: color,
        backgroundColor: color + '22',
        borderWidth: 1.5,
        pointRadius: 0,
        tension: 0.1,
      });
    }
  }

  if (chart) {
    chart.destroy();
  }

  const ctx = document.getElementById('exp-chart').getContext('2d');
  chart = new Chart(ctx, {
    type: 'line',
    data: { datasets },
    options: {
      animation: false,
      parsing: false,
      scales: {
        x: {
          type: 'time',
          time: { tooltipFormat: 'MMM d, HH:mm' },
          ticks: { color: '#aaa', maxTicksLimit: 10 },
          grid: { color: '#333' },
        },
        y: {
          ticks: { color: '#aaa' },
          grid: { color: '#333' },
        },
      },
      plugins: {
        legend: { labels: { color: '#e0e0e0', boxWidth: 12 } },
        tooltip: {
          mode: 'index',
          intersect: false,
          callbacks: {
            title: items => new Date(items[0].parsed.x).toLocaleString(),
          },
        },
        zoom: {
          pan: { enabled: true, mode: 'x' },
          zoom: {
            wheel: { enabled: true },
            pinch: { enabled: true },
            mode: 'x',
          },
        },
      },
      interaction: { mode: 'index', intersect: false },
    },
  });

  const pts = datasets.reduce((s, d) => s + d.data.length, 0);
  document.getElementById('chart-status').textContent =
    `${pts.toLocaleString()} data points across ${datasets.length} series.`;
}
