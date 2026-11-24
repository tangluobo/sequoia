import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.scene.image.*;
import javafx.css.PseudoClass;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Original connect-tree.css and JavaFX TabPane skin, inert contents and no user profile. */
public class TabStripWindowOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var args=getParameters().getRaw();var pane=new TabPane();
   pane.setTabClosingPolicy(TabPane.TabClosingPolicy.ALL_TABS);pane.setFocusTraversable(false);
   pane.setStyle("-fx-padding: 0; -fx-background-insets: 0; -fx-border-insets: 0; -fx-tab-content-padding: 0;");
   pane.getStylesheets().add(Path.of(args.get(1),"css/connect-tree.css").toUri().toString());
   String[] labels={"生产服务器 SSH","sample_db.items","对象","很长的数据库查询标题 SELECT FROM WHERE"};
   for(int i=0;i<labels.length;i++){
    var tab=new Tab(labels[i],new Pane());
    if(i==1||i==2){var image=new ImageView(new Image(Path.of(args.get(1),"images/connect/"+(i==1?"table.png":"folder.png")).toUri().toString()));image.setFitWidth(18);image.setFitHeight(18);image.setPreserveRatio(true);var graphic=new StackPane(image);graphic.setMinSize(18,18);graphic.setPrefSize(18,18);graphic.setMaxSize(18,18);tab.setGraphic(graphic);}
    if(i==2)tab.setClosable(false);pane.getTabs().add(tab);
   }
   pane.getSelectionModel().select(Integer.parseInt(args.get(2)));
   stage.setScene(new Scene(pane,Integer.getInteger("tomato.oracle.width",900),100));stage.setOpacity(.01);stage.show();
   var pause=new PauseTransition(Duration.millis(300));pause.setOnFinished(event->{try{
    pane.applyCss();pane.layout();
    if(args.size()>3&&args.get(3).equals("popup")){pane.lookup(".tab-down-button").fireEvent(new javafx.scene.input.MouseEvent(javafx.scene.input.MouseEvent.MOUSE_CLICKED,4,8,4,8,javafx.scene.input.MouseButton.PRIMARY,1,false,false,false,false,true,false,false,false,false,false,null));}
    else if(args.size()>3){var headers=pane.lookupAll(".tab").stream().filter(n->n.getParent().getStyleClass().contains("headers-region")).sorted(java.util.Comparator.comparingDouble(n->n.localToScene(n.getLayoutBounds()).getMinX())).toList();var header=headers.get(1);header.pseudoClassStateChanged(PseudoClass.getPseudoClass("hover"),true);if(args.get(3).equals("close"))header.lookup(".tab-close-button").pseudoClassStateChanged(PseudoClass.getPseudoClass("hover"),true);pane.applyCss();pane.layout();}
    var path=Path.of(args.get(0));ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();var nodes=new JsonArray();walk(pane,nodes);result.add("nodes",nodes);result.addProperty("networkRequested",false);Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));for(var window:javafx.stage.Window.getWindows())if(window instanceof javafx.stage.PopupWindow){var root=window.getScene().getRoot();root.applyCss();root.layout();ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",Path.of(path+".popup.png").toFile());var popupNodes=new JsonArray();walk(root,popupNodes);Files.writeString(Path.of(path+".popup.json"),new GsonBuilder().setPrettyPrinting().create().toJson(popupNodes));}
    stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
  }
 }
 static void walk(Node node,JsonArray nodes){if(node.getStyleClass().stream().anyMatch(s->java.util.Set.of("tab","tab-label","tab-close-button","tab-header-area","tab-container","control-buttons-tab","tab-down-button","arrow","menu-item","label","radio").contains(s))){var b=node.localToScene(node.getLayoutBounds());var item=new JsonObject();item.addProperty("style",node.getStyleClass().toString());if(node instanceof Labeled l){item.addProperty("text",l.getText());item.addProperty("font",l.getFont().toString());var resource=((com.sun.javafx.font.PGFont)com.sun.javafx.scene.text.FontHelper.getNativeFont(l.getFont())).getFontResource();if(resource instanceof com.sun.javafx.font.CompositeFontResource composite)resource=composite.getSlotResource(0);item.addProperty("physicalFont",resource.getFullName());item.addProperty("fontFile",resource.getFileName());}item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());nodes.add(item);}if(node instanceof Parent p)for(var child:p.getChildrenUnmodifiable())walk(child,nodes);}
}
