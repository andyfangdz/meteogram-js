const { fetchWeatherApi } = require('openmeteo');

async function testECMWFAvailable() {
  console.log('Testing what variables are available for ECMWF IFS...');
  
  // Test with basic variables first
  const basicParams = {
    cell_selection: "nearest",
    latitude: 40.73443,
    longitude: -73.41639,
    models: "ecmwf_ifs",
    hourly: "temperature_2m,relative_humidity_2m,pressure_msl,cloud_cover",
    forecast_hourly: 24,
    timezone: "America/New_York"
  };

  try {
    console.log('Testing basic variables...');
    const responses = await fetchWeatherApi("https://api.open-meteo.com/v1/forecast", basicParams);
    
    if (responses && responses[0]) {
      const response = responses[0];
      const hourly = response.hourly();
      if (hourly) {
        console.log('Basic variables test:');
        for (let i = 0; i < 4; i++) {
          const variable = hourly.variables(i);
          if (variable) {
            const valuesArray = variable.valuesArray();
            const validValues = valuesArray ? valuesArray.filter(v => !isNaN(v)) : [];
            console.log(`  Variable ${i}: ${variable.variable()}, Valid values: ${validValues.length}`);
          }
        }
      }
    }
  } catch (error) {
    console.error('Error with basic variables:', error.message);
  }

  // Test with different pressure levels that might be available
  const pressureLevels = [1000, 925, 850, 700, 500, 300, 250, 200, 150, 100, 50];
  
  for (const level of pressureLevels) {
    console.log(`\n--- Testing pressure level ${level}hPa ---`);
    
    const params = {
      cell_selection: "nearest",
      latitude: 40.73443,
      longitude: -73.41639,
      models: "ecmwf_ifs",
      hourly: `cloud_cover_${level}hPa`,
      forecast_hourly: 24,
      timezone: "America/New_York"
    };

    try {
      const responses = await fetchWeatherApi("https://api.open-meteo.com/v1/forecast", params);
      
      if (responses && responses[0]) {
        const response = responses[0];
        const hourly = response.hourly();
        if (hourly) {
          const variable = hourly.variables(0);
          if (variable) {
            const valuesArray = variable.valuesArray();
            const validValues = valuesArray ? valuesArray.filter(v => !isNaN(v)) : [];
            console.log(`  Cloud cover ${level}hPa: Valid values: ${validValues.length}`);
            if (validValues.length > 0) {
              console.log(`  First few values:`, validValues.slice(0, 3));
            }
          }
        }
      }
    } catch (error) {
      console.error(`Error for level ${level}:`, error.message);
    }
  }
}

testECMWFAvailable();