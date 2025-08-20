// 3D Quickhull Algorithm for computing convex hulls
// Implemented in Plush programming language

// Helper function for absolute value since floats don't have abs() method
fun abs(value) {
    if (value < 0.0) {
        return -value;
    } else {
        return value;
    }
}

class Point3D {
    init(self, x, y, z) {
        self.x = x;
        self.y = y;
        self.z = z;
    }
    
    to_s(self) {
        return "Point3D(" + self.x.to_s() + ", " + self.y.to_s() + ", " + self.z.to_s() + ")";
    }
    
    subtract(self, other) {
        return Point3D(self.x - other.x, self.y - other.y, self.z - other.z);
    }
    
    dot(self, other) {
        return self.x * other.x + self.y * other.y + self.z * other.z;
    }
    
    cross(self, other) {
        return Point3D(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x
        );
    }
    
    magnitude(self) {
        return (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
    }
    
    normalize(self) {
        let mag = self.magnitude();
        if (mag == 0.0) {
            return Point3D(0.0, 0.0, 0.0);
        }
        return Point3D(self.x / mag, self.y / mag, self.z / mag);
    }

    check(self, reference) {
        // these should really be comparing w/ epsilon
        assert(self.x == reference.x);
        assert(self.y == reference.y);
        assert(self.z == reference.z);
    }
}

class Face {
    init(self, p1, p2, p3) {
        self.vertices = [p1, p2, p3];
        self.normal = self.compute_normal();
        self.visible_points = [];
    }
    
    compute_normal(self) {
        let v1 = self.vertices[1].subtract(self.vertices[0]);
        let v2 = self.vertices[2].subtract(self.vertices[0]);
        return v1.cross(v2).normalize();
    }
    
    distance_to_point(self, point) {
        let v = point.subtract(self.vertices[0]);
        return v.dot(self.normal);
    }
    
    is_visible_from(self, point) {
        return self.distance_to_point(point) > 0.0001; // Small epsilon for numerical stability
    }
    
    to_s(self) {
        return "Face(" + self.vertices[0].to_s() + ", " + 
               self.vertices[1].to_s() + ", " + self.vertices[2].to_s() + ")";
    }

    check_face(self, reference) {
        self.vertices[0].check(reference.vertices[0]);
        self.vertices[1].check(reference.vertices[1]);
        self.vertices[2].check(reference.vertices[2]);
    }
}

class Edge {
    init(self, p1, p2) {
        self.vertices = [p1, p2];
    }
    
    equals(self, other) {
        return (self.vertices[0] == other.vertices[0] && self.vertices[1] == other.vertices[1]) ||
               (self.vertices[0] == other.vertices[1] && self.vertices[1] == other.vertices[0]);
    }
    
    to_s(self) {
        return "Edge[" + self.vertices[0].to_s() + " -> " + self.vertices[1].to_s() + "]";
    }
}

class Quickhull3D {
    init(self, points) {
        self.points = points;
        self.faces = [];
        self.epsilon = 0.0001;
    }
    
    compute_hull(self) {
        if (self.points.len < 4) {
            $println("Error: Need at least 4 points for 3D convex hull");
            return [];
        }
        
        // Find initial tetrahedron
        let initial_tetrahedron = self.find_initial_tetrahedron();
        if (initial_tetrahedron == nil) {
            $println("Error: Could not find initial tetrahedron");
            return [];
        }
        
        // Create initial faces
        self.create_initial_faces(initial_tetrahedron);
        
        // Assign remaining points to visible faces
        self.assign_points_to_faces();
        
        // Process each face with visible points
        let var face_idx = 0;
        while (face_idx < self.faces.len) {
            let face = self.faces[face_idx];
            if (face.visible_points.len > 0) {
                self.process_face(face_idx);
                // Start over since we may have added new faces
                face_idx = 0;
            } else {
                face_idx = face_idx + 1;
            }
        }
        
        return self.faces;
    }
    
    find_initial_tetrahedron(self) {
        // Find 4 points that form a non-degenerate tetrahedron
        // For simplicity, we'll use the first 4 points that aren't coplanar
        if (self.points.len < 4) {
            return nil;
        }
        
        for (let var i = 0; i < self.points.len - 3; ++i) {
            for (let var j = i + 1; j < self.points.len - 2; ++j) {
                for (let var k = j + 1; k < self.points.len - 1; ++k) {
                    for (let var l = k + 1; l < self.points.len; ++l) {
                        let p1 = self.points[i];
                        let p2 = self.points[j];
                        let p3 = self.points[k];
                        let p4 = self.points[l];
                        
                        // Check if points form a valid tetrahedron
                        if (self.is_valid_tetrahedron(p1, p2, p3, p4)) {
                            return [p1, p2, p3, p4];
                        }
                    }
                }
            }
        }
        return nil;
    }
    
    is_valid_tetrahedron(self, p1, p2, p3, p4) {
        // Check if 4 points form a non-degenerate tetrahedron
        let v1 = p2.subtract(p1);
        let v2 = p3.subtract(p1);
        let v3 = p4.subtract(p1);
        
        // Compute scalar triple product (volume)
        let volume = abs(v1.dot(v2.cross(v3)));
        return volume > self.epsilon;
    }
    
    create_initial_faces(self, tetrahedron) {
        // Create 4 faces of the tetrahedron
        let p1 = tetrahedron[0];
        let p2 = tetrahedron[1];
        let p3 = tetrahedron[2];
        let p4 = tetrahedron[3];
        
        // Ensure faces are oriented outward
        self.faces.push(Face(p1, p2, p3));
        self.faces.push(Face(p1, p3, p4));
        self.faces.push(Face(p1, p4, p2));
        self.faces.push(Face(p2, p4, p3));
        
        // Fix orientation of faces
        self.fix_face_orientations(tetrahedron);
    }
    
    fix_face_orientations(self, tetrahedron) {
        // Ensure all face normals point outward
        let center = self.compute_centroid(tetrahedron);
        
        for (let var i = 0; i < self.faces.len; ++i) {
            let face = self.faces[i];
            let face_center = self.compute_face_center(face);
            let to_center = center.subtract(face_center);
            
            // If normal points toward center, flip the face
            if (face.normal.dot(to_center) > 0.0) {
                self.flip_face(face);
            }
        }
    }
    
    compute_centroid(self, points) {
        let var sum_x = 0.0;
        let var sum_y = 0.0;
        let var sum_z = 0.0;
        
        for (let var i = 0; i < points.len; ++i) {
            sum_x = sum_x + points[i].x;
            sum_y = sum_y + points[i].y;
            sum_z = sum_z + points[i].z;
        }
        
        let n = points.len.to_f();
        return Point3D(sum_x / n, sum_y / n, sum_z / n);
    }
    
    compute_face_center(self, face) {
        let var sum_x = 0.0;
        let var sum_y = 0.0;
        let var sum_z = 0.0;
        
        for (let var i = 0; i < face.vertices.len; ++i) {
            sum_x = sum_x + face.vertices[i].x;
            sum_y = sum_y + face.vertices[i].y;
            sum_z = sum_z + face.vertices[i].z;
        }
        
        return Point3D(sum_x / 3.0, sum_y / 3.0, sum_z / 3.0);
    }
    
    flip_face(self, face) {
        // Swap vertices to flip normal direction
        let temp = face.vertices[1];
        face.vertices[1] = face.vertices[2];
        face.vertices[2] = temp;
        face.normal = face.compute_normal();
    }
    
    assign_points_to_faces(self) {
        for (let var i = 0; i < self.points.len; ++i) {
            let point = self.points[i];
            let var assigned = false;
            
            // Check if point is a vertex of any face
            for (let var j = 0; j < self.faces.len && !assigned; ++j) {
                let face = self.faces[j];
                for (let var k = 0; k < face.vertices.len; ++k) {
                    if (self.points_equal(point, face.vertices[k])) {
                        assigned = true;
                    }
                }
            }
            
            if (!assigned) {
                // Find the face from which this point is most visible
                let var best_face = -1;
                let var max_distance = 0.0;
                
                for (let var j = 0; j < self.faces.len; ++j) {
                    let face = self.faces[j];
                    if (face.is_visible_from(point)) {
                        let distance = face.distance_to_point(point);
                        if (distance > max_distance) {
                            max_distance = distance;
                            best_face = j;
                        }
                    }
                }
                
                if (best_face >= 0) {
                    self.faces[best_face].visible_points.push(point);
                }
            }
        }
    }
    
    points_equal(self, p1, p2) {
        return abs(p1.x - p2.x) < self.epsilon &&
               abs(p1.y - p2.y) < self.epsilon &&
               abs(p1.z - p2.z) < self.epsilon;
    }
    
    process_face(self, face_idx) {
        let face = self.faces[face_idx];
        if (face.visible_points.len == 0) {
            return;
        }
        
        // Find the farthest point
        let farthest_point = self.find_farthest_point(face);
        
        // Find all faces visible from the farthest point
        let visible_faces = self.find_visible_faces(farthest_point);
        
        // Create horizon edges
        let horizon_edges = self.find_horizon_edges(visible_faces);
        
        // Remove visible faces and collect their visible points
        let orphaned_points = [];
        for (let var i = visible_faces.len - 1; i >= 0; --i) {
            let visible_face_idx = visible_faces[i];
            let visible_face = self.faces[visible_face_idx];
            
            // Collect orphaned points
            for (let var j = 0; j < visible_face.visible_points.len; ++j) {
                let point = visible_face.visible_points[j];
                if (!self.points_equal(point, farthest_point)) {
                    orphaned_points.push(point);
                }
            }
            
            // Remove face
            self.remove_face(visible_face_idx);
        }
        
        // Create new faces from horizon edges to farthest point
        for (let var i = 0; i < horizon_edges.len; ++i) {
            let edge = horizon_edges[i];
            let new_face = Face(edge.vertices[0], edge.vertices[1], farthest_point);
            self.faces.push(new_face);
        }
        
        // Reassign orphaned points to new faces
        self.reassign_orphaned_points(orphaned_points);
    }
    
    find_farthest_point(self, face) {
        let var farthest = face.visible_points[0];
        let var max_distance = face.distance_to_point(farthest);
        
        for (let var i = 1; i < face.visible_points.len; ++i) {
            let point = face.visible_points[i];
            let distance = face.distance_to_point(point);
            if (distance > max_distance) {
                max_distance = distance;
                farthest = point;
            }
        }
        
        return farthest;
    }
    
    find_visible_faces(self, point) {
        let visible = [];
        for (let var i = 0; i < self.faces.len; ++i) {
            if (self.faces[i].is_visible_from(point)) {
                visible.push(i);
            }
        }
        return visible;
    }
    
    find_horizon_edges(self, visible_face_indices) {
        let horizon = [];
        
        // For each visible face, check its edges
        for (let var i = 0; i < visible_face_indices.len; ++i) {
            let face_idx = visible_face_indices[i];
            let face = self.faces[face_idx];
            
            // Check each edge of this face
            for (let var j = 0; j < 3; ++j) {
                let v1 = face.vertices[j];
                let v2 = face.vertices[(j + 1) % 3];
                let edge = Edge(v1, v2);
                
                // Check if this edge is shared with any other visible face
                let var is_shared = false;
                for (let var k = 0; k < visible_face_indices.len && !is_shared; ++k) {
                    if (k != i) {
                        let other_face_idx = visible_face_indices[k];
                        let other_face = self.faces[other_face_idx];
                        is_shared = self.face_contains_edge(other_face, edge);
                    }
                }
                
                // If edge is not shared with other visible faces, it's a horizon edge
                if (!is_shared) {
                    horizon.push(edge);
                }
            }
        }
        
        return horizon;
    }
    
    face_contains_edge(self, face, edge) {
        for (let var i = 0; i < 3; ++i) {
            let v1 = face.vertices[i];
            let v2 = face.vertices[(i + 1) % 3];
            let face_edge = Edge(v1, v2);
            if (edge.equals(face_edge)) {
                return true;
            }
        }
        return false;
    }
    
    remove_face(self, face_idx) {
        // Remove face by shifting all subsequent faces down
        for (let var i = face_idx; i < self.faces.len - 1; ++i) {
            self.faces[i] = self.faces[i + 1];
        }
        self.faces.pop();
    }
    
    reassign_orphaned_points(self, orphaned_points) {
        for (let var i = 0; i < orphaned_points.len; ++i) {
            let point = orphaned_points[i];
            let var best_face = -1;
            let var max_distance = 0.0;
            
            for (let var j = 0; j < self.faces.len; ++j) {
                let face = self.faces[j];
                if (face.is_visible_from(point)) {
                    let distance = face.distance_to_point(point);
                    if (distance > max_distance) {
                        max_distance = distance;
                        best_face = j;
                    }
                }
            }
            
            if (best_face >= 0) {
                self.faces[best_face].visible_points.push(point);
            }
        }
    }
    
    print_hull(self) {
        for (let var i = 0; i < self.faces.len; ++i) {
            $println(self.faces[i].to_s());
        }
    }

    check_hull(self, reference) {
        assert(self.faces.len == reference.len);
        for (let var i = 0; i < self.faces.len; ++i) {
            self.faces[i].check_face(reference[i]);
        }
    }
}

// Test functions
fun create_test_points_cube() {
    // Create vertices of a cube
    return [
        Point3D(0.0, 0.0, 0.0),
        Point3D(1.0, 0.0, 0.0),
        Point3D(1.0, 1.0, 0.0),
        Point3D(0.0, 1.0, 0.0),
        Point3D(0.0, 0.0, 1.0),
        Point3D(1.0, 0.0, 1.0),
        Point3D(1.0, 1.0, 1.0),
        Point3D(0.0, 1.0, 1.0)
    ];
}

fun create_test_points_random() {
    // Create some random points (including some interior points)
    return [
        Point3D(0.0, 0.0, 0.0),
        Point3D(3.0, 0.0, 0.0),
        Point3D(0.0, 3.0, 0.0),
        Point3D(0.0, 0.0, 3.0),
        Point3D(1.0, 1.0, 1.0),  // Interior point
        Point3D(2.0, 2.0, 0.0),
        Point3D(1.5, 0.5, 2.5),
        Point3D(-1.0, 1.0, 1.0)
    ];
}

fun main() {
    // Test case 1: Cube
    let cube_points = create_test_points_cube();
    let quickhull1 = Quickhull3D(cube_points);
    let hull1 = quickhull1.compute_hull();
    quickhull1.check_hull([
Face(Point3D(0.0, 0.0, 0.0), Point3D(1.0, 1.0, 0.0), Point3D(1.0, 0.0, 0.0)),
Face(Point3D(0.0, 0.0, 0.0), Point3D(1.0, 0.0, 0.0), Point3D(0.0, 0.0, 1.0)),
Face(Point3D(0.0, 0.0, 0.0), Point3D(0.0, 0.0, 1.0), Point3D(0.0, 1.0, 0.0)),
Face(Point3D(1.0, 1.0, 0.0), Point3D(0.0, 0.0, 0.0), Point3D(0.0, 1.0, 0.0)),
Face(Point3D(1.0, 0.0, 0.0), Point3D(1.0, 1.0, 0.0), Point3D(1.0, 0.0, 1.0)),
Face(Point3D(0.0, 0.0, 1.0), Point3D(1.0, 0.0, 0.0), Point3D(1.0, 0.0, 1.0)),
Face(Point3D(1.0, 1.0, 0.0), Point3D(0.0, 1.0, 0.0), Point3D(0.0, 1.0, 1.0)),
Face(Point3D(0.0, 1.0, 0.0), Point3D(0.0, 0.0, 1.0), Point3D(0.0, 1.0, 1.0)),
Face(Point3D(0.0, 0.0, 1.0), Point3D(1.0, 0.0, 1.0), Point3D(0.0, 1.0, 1.0)),
Face(Point3D(1.0, 0.0, 1.0), Point3D(1.0, 1.0, 0.0), Point3D(1.0, 1.0, 1.0)),
Face(Point3D(1.0, 1.0, 0.0), Point3D(0.0, 1.0, 1.0), Point3D(1.0, 1.0, 1.0)),
Face(Point3D(0.0, 1.0, 1.0), Point3D(1.0, 0.0, 1.0), Point3D(1.0, 1.0, 1.0))
    ]);
    
    // Test case 2: Random points
    let random_points = create_test_points_random();
    let quickhull2 = Quickhull3D(random_points);
    let hull2 = quickhull2.compute_hull();
    quickhull2.check_hull([
Face(Point3D(0.0, 0.0, 0.0), Point3D(0.0, 3.0, 0.0), Point3D(3.0, 0.0, 0.0)),
Face(Point3D(0.0, 0.0, 0.0), Point3D(3.0, 0.0, 0.0), Point3D(0.0, 0.0, 3.0)),
Face(Point3D(0.0, 0.0, 0.0), Point3D(0.0, 0.0, 3.0), Point3D(-1.0, 1.0, 1.0)),
Face(Point3D(0.0, 0.0, 3.0), Point3D(0.0, 3.0, 0.0), Point3D(-1.0, 1.0, 1.0)),
Face(Point3D(0.0, 3.0, 0.0), Point3D(0.0, 0.0, 0.0), Point3D(-1.0, 1.0, 1.0)),
Face(Point3D(0.0, 3.0, 0.0), Point3D(0.0, 0.0, 3.0), Point3D(1.5, 0.5, 2.5)),
Face(Point3D(0.0, 0.0, 3.0), Point3D(3.0, 0.0, 0.0), Point3D(1.5, 0.5, 2.5)),
Face(Point3D(3.0, 0.0, 0.0), Point3D(0.0, 3.0, 0.0), Point3D(2.0, 2.0, 0.0)),
Face(Point3D(0.0, 3.0, 0.0), Point3D(1.5, 0.5, 2.5), Point3D(2.0, 2.0, 0.0)),
Face(Point3D(1.5, 0.5, 2.5), Point3D(3.0, 0.0, 0.0), Point3D(2.0, 2.0, 0.0))
    ]);
}

// Run the main function
main();
