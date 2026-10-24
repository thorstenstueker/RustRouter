// Java harness used to verify the Rust DSN/SES/rules I/O against the original.
// Build: javac -proc:none -cp <freerouting classes or jar> Dump.java
// Usage: java Dump <dsn> <ses_out> [rules_out] [rules_in or ""] [ses_in]
// The output format equals io::specctra::tests::dump_board (Rust).
import app.freerouting.board.facade.BasicBoard;
import app.freerouting.board.model.items.*;
import app.freerouting.board.model.structure.BoardOutline;
import app.freerouting.geometry.planar.*;
import app.freerouting.io.BoardReadResult;
import app.freerouting.io.specctra.DsnReader;
import app.freerouting.io.specctra.SesWriter;
import app.freerouting.rules.*;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.library.*;
import java.io.*;
import java.util.*;

public class Dump {
  static String box(IntBox b) { return b.isEmpty() ? "[empty]" : "[" + b.ll.x + "," + b.ll.y + "," + b.ur.x + "," + b.ur.y + "]"; }
  static String pt(Point p) {
    if (p instanceof IntPoint ip) return "(" + ip.x + "," + ip.y + ")";
    FloatPoint f = p.toFloat(); return "(~" + f.x + ",~" + f.y + ")";
  }
  static String shape(Shape s) {
    if (s == null) return "null";
    String kind = s instanceof IntBox ? "B" : s instanceof IntOctagon ? "O" : s instanceof Simplex ? "S" : s instanceof Circle ? "C" : s instanceof PolygonShape ? "P" : "?";
    int n = (s instanceof Circle) ? 0 : ((PolylineShape) s).borderLineCount();
    return kind + n + box(s.boundingBox());
  }
  public static void main(String[] a) throws Exception {
    String path = a[0];
    BoardReadResult r = DsnReader.readBoard(new FileInputStream(path), null, null, path);
    BasicBoard b = null;
    if (r instanceof BoardReadResult.Success s) b = s.board();
    else if (r instanceof BoardReadResult.OutlineMissing o) { System.out.println("RESULT OutlineMissing"); b = o.board(); }
    else { System.out.println("RESULT " + r); return; }
    if (a.length > 3 && b != null && !a[3].isEmpty()) {
      boolean ok = app.freerouting.io.specctra.RulesReader.read(new FileInputStream(a[3]), "x", b);
      System.out.println("RULESREAD " + ok);
    }
    if (a.length > 4 && b != null) {
      try { var sum = app.freerouting.io.specctra.SesReader.read(new FileInputStream(a[4]), b);
        System.out.println("SESREAD " + sum.wiresImported() + " " + sum.viasImported() + " " + sum.errorsEncountered());
      } catch (Exception e) { System.out.println("SESERROR " + e.getMessage()); return; }
    }
    PrintStream o = System.out;
    if (b == null) { o.println("NO BOARD"); return; }
    o.println("bbox " + box(b.getBoundingBox()));
    o.println("scale " + b.communication.coordinateTransform.dsnToBoard(1) + " unit " + b.communication.unit + " res " + b.communication.resolution);
    for (int i = 0; i < b.layerStructure.layers.length; i++) o.println("layer " + i + " " + b.layerStructure.layers[i].name + " " + b.layerStructure.layers[i].isSignal);
    BoardRules ru = b.rules;
    o.println("rules angle=" + ru.getTraceAngleRestriction() + " pin_edge=" + ru.getPinEdgeToTurnDist() + " minhw=" + ru.getMinTraceHalfWidth() + " maxhw=" + ru.getMaxTraceHalfWidth());
    ClearanceMatrix cm = ru.clearanceMatrix;
    for (int i = 0; i < cm.getClassCount(); i++) {
      StringBuilder sb = new StringBuilder("cl " + i + " " + cm.getName(i) + ":");
      for (int j = 0; j < cm.getClassCount(); j++) { sb.append(" "); for (int l = 0; l < cm.getLayerCount(); l++) sb.append(l > 0 ? "/" : "").append(cm.getValue(i, j, l, false)); }
      o.println(sb);
    }
    List<NetClass> ncs = new ArrayList<>();
    for (int i = 0; i < ru.netClasses.count(); i++) ncs.add(ru.netClasses.get(i));
    for (int i = 0; i < ncs.size(); i++) {
      NetClass c = ncs.get(i);
      StringBuilder sb = new StringBuilder("netclass " + i + " " + c.getName() + " hw=");
      for (int l = 0; l < c.layerCount(); l++) sb.append(l > 0 ? "/" : "").append(c.getTraceHalfWidth(l));
      sb.append(" cl=" + c.getTraceClearanceClass() + " via_rule=" + (c.getViaRule() == null ? "null" : c.getViaRule().name));
      sb.append(" active=");
      for (int l = 0; l < c.layerCount(); l++) sb.append(c.isActiveRoutingLayer(l) ? "1" : "0");
      sb.append(" dicc=");
      for (DefaultItemClearanceClasses.ItemClass ic : DefaultItemClearanceClasses.ItemClass.values()) sb.append(c.defaultItemClearanceClasses.get(ic)).append(",");
      sb.append(" pull=" + c.getPullTight() + " shove=" + c.isShoveFixed() + " minl=" + c.getMinimumTraceLength() + " maxl=" + c.getMaximumTraceLength());
      o.println(sb);
    }
    for (int i = 1; i <= ru.nets.maxNetNumber(); i++) {
      Net n = ru.nets.get(i);
      o.println("net " + n.netNumber + " " + n.name + " " + n.subnetNumber + " class=" + ncs.indexOf(n.getNetClass()) + " plane=" + n.containsPlane());
    }
    for (int i = 0; i < ru.viaInfos.count(); i++) { ViaInfo v = ru.viaInfos.get(i); o.println("viainfo " + v.getName() + " " + v.getPadstack().name + " " + v.getClearanceClassIndex() + " " + v.attachSmdAllowed()); }
    for (ViaRule vr : ru.viaRules) { StringBuilder sb = new StringBuilder("viarule " + vr.name + ":"); for (int i = 0; i < vr.viaCount(); i++) sb.append(" " + vr.getVia(i).getName()); o.println(sb); }
    for (int i = 1; b.library.padstacks != null && i <= b.library.padstacks.count(); i++) {
      Padstack p = b.library.padstacks.get(i);
      StringBuilder sb = new StringBuilder("padstack " + p.id + " " + p.name + " attach=" + p.attachAllowed + " abs=" + p.placedAbsolute + " nonconvex=" + p.hasNonConvexGeometry + ":");
      for (int l = 0; l < b.getLayerCount(); l++) { sb.append(" ").append(shape(p.getShape(l))); ConvexShape[] aux = p.getAuxiliaryShapes(l); if (aux != null) sb.append("+aux" + aux.length); }
      o.println(sb);
    }
    StringBuilder vps = new StringBuilder("via_padstacks:");
    for (int i = 0; i < b.library.viaPadstackCount(); i++) vps.append(" " + b.library.getViaPadstack(i).name);
    o.println(vps);
    for (int i = 1; b.library.packages != null && i <= b.library.packages.count(); i++) {
      app.freerouting.core.library.Package p = b.library.packages.get(i);
      StringBuilder sb = new StringBuilder("package " + p.id + " " + p.name + " front=" + p.isFront + " outlines=" + p.outline.length + " keepouts=" + p.keepouts.length + "/" + p.viaKeepouts.length + "/" + p.placeKeepoutArr.length + ":");
      for (int k = 0; k < p.pinCount(); k++) { var pin = p.getPin(k); FloatPoint e = pin.getExactRelativeLocation(); sb.append(" " + pin.name + "/" + pin.padstackId + "@" + e.x + "," + e.y + " r" + pin.rotationInDegree); }
      o.println(sb);
    }
    for (int i = 1; i <= b.components.count(); i++) {
      var c = b.components.get(i);
      FloatPoint e = c.getExactLocation();
      o.println("component " + c.id + " " + c.name + " pkg=" + c.getPackage().id + " " + (e == null ? "unplaced" : e.x + "," + e.y) + " rot=" + c.getRotationInDegree() + " front=" + c.placedOnFront() + " fixed=" + c.positionFixed + " lp=" + (c.getLogicalPart() == null ? "-" : c.getLogicalPart().name));
    }
    int count = 0;
    for (Item it : b.getItems()) {
      count++;
      StringBuilder sb = new StringBuilder("item " + it.getId() + " " + it.getBoardItemType() + " nets=[");
      for (int k = 0; k < it.netCount(); k++) sb.append(k > 0 ? "," : "").append(it.getNetNumber(k));
      sb.append("] cl=" + it.clearanceClassIndex() + " comp=" + it.getComponentId() + " fix=" + it.getFixedState() + " L=" + it.firstLayer() + "-" + it.lastLayer() + " bb=" + box(it.boundingBox()));
      if (it instanceof DrillItem d) sb.append(" c=" + pt(d.getCenter()) + " ps=" + d.getPadstack().id);
      if (it instanceof Pin p) sb.append(" pin=" + p.getPinIndex());
      if (it instanceof PolylineTrace t) { sb.append(" hw=" + t.getHalfWidth() + " corners="); for (Point p : t.polyline().corners()) sb.append(pt(p)); }
      if (it instanceof ObstacleArea oa) sb.append(" name=" + oa.name);
      if (it instanceof BoardOutline bo) sb.append(" shapes=" + bo.shapeCount());
      sb.append(" tiles=" + it.tileShapeCount() + ":");
      for (int k = 0; k < it.tileShapeCount(); k++) sb.append(" ").append(shape(it.getTileShape(k)));
      o.println(sb);
    }
    o.println("itemcount " + count);
    if (a.length > 1) {
      try (FileOutputStream fo = new FileOutputStream(a[1])) { SesWriter.write(b, fo, new File(path).getName()); } catch (Exception e) { System.out.println("SES EXCEPTION " + e); }
      if (a.length > 2) { try (FileOutputStream fo = new FileOutputStream(a[2])) { app.freerouting.io.specctra.RulesWriter.write(b, fo, "design"); } }
    }
  }
}
