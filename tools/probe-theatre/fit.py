import sys, numpy as np
from pyproj import Transformer
name = sys.argv[1]
import os
rows = np.loadtxt(os.path.join(os.path.dirname(os.path.abspath(__file__)), f"{name}.txt"))
x, z, lat, lon = rows.T
best = None
for lon0 in np.arange(int(lon.min())-15, int(lon.max())+16, 1):
    for k0 in (0.9996,):
        t = Transformer.from_crs("EPSG:4326", f"+proj=tmerc +lat_0=0 +lon_0={lon0} +k_0={k0} +x_0=0 +y_0=0 +datum=WGS84 +units=m", always_xy=True)
        e, n = t.transform(lon, lat)
        # dcs_z = e + x0 ; dcs_x = n + y0  -> offsets
        x0 = np.mean(z - e); y0 = np.mean(x - n)
        res = np.hypot(z - (e + x0), x - (n + y0))
        if best is None or res.max() < best[0]:
            best = (res.max(), res.mean(), lon0, k0, x0, y0)
rmax, rmean, lon0, k0, x0, y0 = best
print(f"{name}: lon_0={lon0} k_0={k0} easting_offset={-x0:.3f} northing_offset={-y0:.3f}  max_resid={rmax:.3f} m mean={rmean:.3f} m")
print(f"  proj4: +proj=tmerc +lat_0=0 +lon_0={lon0} +k_0={k0} +x_0={x0:.3f} +y_0={y0:.3f} +datum=WGS84 +units=m")
# also try fitting k_0 freely with lon0 fixed to see if 0.9996 is exact
from scipy.optimize import least_squares
def f(p):
    t = Transformer.from_crs("EPSG:4326", f"+proj=tmerc +lat_0=0 +lon_0={p[0]} +k_0={p[1]} +x_0={p[2]} +y_0={p[3]} +datum=WGS84 +units=m", always_xy=True)
    e, n = t.transform(lon, lat); return np.concatenate([e - z, n - x])
r = least_squares(f, [lon0, k0, x0, y0])
print(f"  free fit: lon_0={r.x[0]:.6f} k_0={r.x[1]:.7f} x_0={r.x[2]:.2f} y_0={r.x[3]:.2f} max_resid={np.abs(r.fun).max():.3f} m")
