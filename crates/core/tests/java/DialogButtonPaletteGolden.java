import javafx.application.*;
import javafx.css.PseudoClass;
import javafx.scene.Scene;
import javafx.scene.control.Button;
import javafx.scene.layout.*;
import javafx.scene.paint.*;
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
/** Resolves the real OpenJFX stock dialog button CSS, without showing a window. */
public class DialogButtonPaletteGolden {
 static JsonArray numbers(double... values){var result=new JsonArray();for(double value:values)result.add(value);return result;}
 static JsonArray color(Color value){return numbers(value.getRed(),value.getGreen(),value.getBlue(),value.getOpacity());}
 public static void main(String[] args){Platform.startup(()->{try{
  var output=new JsonArray();
  var states=new ArrayList<String>();for(boolean isDefault:List.of(false,true))for(boolean focused:List.of(false,true))for(String phase:List.of("normal","hover","armed","armed-hover"))states.add((isDefault?"default-":"")+(focused?"focused-":"")+phase);states.add("disabled");states.add("default-disabled");
  for(String state:states){
   var root=new Pane();root.setStyle("-fx-font-size:12px;");var scene=new Scene(root,418,101);var button=new Button("确定");root.getChildren().add(button);button.setPrefSize(75,23);button.resizeRelocate(10,10,75,23);
   button.setDefaultButton(state.startsWith("default"));button.setDisable(state.endsWith("disabled"));
   for(String pseudo:List.of("hover","armed","focused"))button.pseudoClassStateChanged(PseudoClass.getPseudoClass(pseudo),state.contains(pseudo));
   root.applyCss();root.layout();
   var row=new JsonObject();row.addProperty("state",state);row.addProperty("opacity",button.getOpacity());row.addProperty("fontName",button.getFont().getName());row.addProperty("fontSize",button.getFont().getSize());row.add("text",color((Color)button.getTextFill()));var fills=new JsonArray();
   for(var fill:button.getBackground().getFills()){
    var value=new JsonObject();var inset=fill.getInsets();value.add("insets",numbers(inset.getTop(),inset.getRight(),inset.getBottom(),inset.getLeft()));var radius=fill.getRadii();value.add("radii",numbers(radius.getTopLeftHorizontalRadius(),radius.getTopRightHorizontalRadius(),radius.getBottomRightHorizontalRadius(),radius.getBottomLeftHorizontalRadius()));
    if(fill.getFill() instanceof Color solid){value.add("solid",color(solid));}
    else if(fill.getFill() instanceof LinearGradient gradient){value.add("direction",numbers(gradient.getStartX(),gradient.getStartY(),gradient.getEndX(),gradient.getEndY()));value.addProperty("proportional",gradient.isProportional());var stops=new JsonArray();for(var stop:gradient.getStops()){var entry=new JsonObject();entry.addProperty("offset",stop.getOffset());entry.add("color",color(stop.getColor()));stops.add(entry);}value.add("stops",stops);}
    else throw new AssertionError("Unexpected stock button paint: "+fill.getFill());fills.add(value);
   }
   row.add("fills",fills);output.add(row);
  }
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().create().toJson(output));Platform.exit();System.exit(0);
 }catch(Throwable error){error.printStackTrace();System.exit(1);}});}
}
